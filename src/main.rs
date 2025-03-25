use std::fs::{self, DirEntry, File};
use std::io::{BufRead, BufReader, Result, Seek, Write};
use std::path::Path;
use std::process::{self, ChildStderr, ChildStdout, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime};

use nix::sys::reboot::RebootMode;
use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal as Sig};
use sys_mount::{Mount, Unmount, UnmountDrop, UnmountFlags};
use sysinfo::{ProcessExt, Signal, System, SystemExt};

const SERVICE_RESTART_INTERVAL: Duration = Duration::from_secs(30);

macro_rules! log {
    ($col:expr, $($tts:tt)*) => {
        {
            println!($($tts)*);
        }
    };
}

macro_rules! log_raw {
    ($col:expr, $($tts:tt)*) => {
        {
            print!($($tts)*);
        }
    };
}

macro_rules! halt {
    () => {
        loop {
            thread::park();
        }
    };
}

fn start() -> Result<()> {
    log!(Color::Yellow, "Starting rustkrazy");

    for service in fs::read_dir("/bin")? {
        let service = service?;
        let service_name = match service.file_name().into_string() {
            Ok(v) => v,
            Err(_) => {
                log!(Color::Red, "[ ERROR ] invalid unicode in file name");
                continue;
            }
        };

        if service_name == "init" {
            continue;
        }

        thread::spawn(move || match supervise(service, service_name.clone()) {
            Ok(_) => {}
            Err(e) => log!(Color::Red, "can't supervise {}: {}", service_name, e),
        });
    }

    Ok(())
}

fn supervise(service: DirEntry, service_name: String) -> Result<()> {
    loop {
        let mut cmd = Command::new(service.path());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        match cmd.spawn() {
            Ok(mut child) => {
                log!(Color::Green, "[  OK   ] starting {}", service_name);

                let child_stdout = child.stdout.take();
                let service_name2 = service_name.clone();
                thread::spawn(move || {
                    log_out(child_stdout.expect("no child stdout"), service_name2)
                        .expect("logging stdout failed");
                });

                let child_stderr = child.stderr.take();
                let service_name2 = service_name.clone();
                thread::spawn(move || {
                    log_err(child_stderr.expect("no child stderr"), service_name2)
                        .expect("logging stderr failed");
                });

                match child.wait() {
                    Ok(status) => {
                        log!(
                            Color::Yellow,
                            "[ INFO  ] {} exited with {}",
                            service_name,
                            status
                        );
                    }
                    Err(e) => {
                        log!(
                            Color::Red,
                            "[ ERROR ] can't wait for {} to exit: {}",
                            service_name,
                            e
                        );
                    }
                }
            }
            Err(e) => {
                log!(Color::Red, "[ ERROR ] starting {}: {}", service_name, e);
            }
        }

        thread::sleep(SERVICE_RESTART_INTERVAL);
    }
}

fn log_out(pipe: ChildStdout, service_name: String) -> Result<()> {
    let mut file = File::create(Path::new("/tmp").join(service_name.clone() + ".log"))?;
    let mut r = BufReader::new(pipe);

    loop {
        let mut buf = String::new();
        if r.read_line(&mut buf)? == 0 {
            log!(Color::Yellow, "[ INFO  ] {} closed stdout", service_name);
            return Ok(());
        }

        if file.metadata()?.len() > 30000000 {
            file.set_len(0)?;
            file.rewind()?;
        }

        if !buf.is_empty() {
            let timestamp = humantime::format_rfc3339_seconds(SystemTime::now());
            let buf = format!("[{} {}] {}", timestamp, service_name, buf);

            log_raw!(Color::White, "{}", buf);

            file.write_all(buf.as_bytes())?;
        }
    }
}

fn log_err(pipe: ChildStderr, service_name: String) -> Result<()> {
    let mut file = File::create(Path::new("/data").join(service_name.clone() + ".err"))?;
    let mut r = BufReader::new(pipe);

    loop {
        let mut buf = String::new();
        if r.read_line(&mut buf)? == 0 {
            log!(Color::Yellow, "[ INFO  ] {} closed stderr", service_name);
            return Ok(());
        }

        if file.metadata()?.len() > 30000000 {
            file.set_len(0)?;
            file.rewind()?;
        }

        if !buf.is_empty() {
            let timestamp = humantime::format_rfc3339_seconds(SystemTime::now());
            let buf = format!("[{} {}] {}", timestamp, service_name, buf);

            log_raw!(Color::White, "{}", buf);

            file.write_all(buf.as_bytes())?;
        }
    }
}

fn mount_or_halt(part_id: u8, mount_point: &str, fs: &str) -> UnmountDrop<Mount> {
    let mut mount = None;
    let mut mount_err = None;

    let devs = [
        format!("/dev/mmcblk0p{}", part_id),
        format!("/dev/sda{}", part_id),
        format!("/dev/vda{}", part_id),
    ];

    for dev in &devs {
        match Mount::builder().fstype(fs).mount(dev, mount_point) {
            Ok(v) => {
                mount = Some(v.into_unmount_drop(UnmountFlags::DETACH));
                break;
            }
            Err(e) => {
                mount_err = Some(e);
            }
        };
    }

    match mount {
        None => {
            if let Some(e) = mount_err {
                log!(Color::Red, "[ ERROR ] can't mount {}: {}", mount_point, e)
            } else {
                log!(
                    Color::Red,
                    "[ ERROR ] can't mount {}: unknown error (this shouldn't happen)",
                    mount_point
                );
            }

            halt!();
        }
        Some(handle) => handle,
    }
}

fn end() {
    log!(Color::Yellow, "[ INFO  ] send SIGTERM to all processes");
    for process in System::new_all().processes().values() {
        process.kill_with(Signal::Term);
    }

    thread::sleep(Duration::from_secs(3));
}

extern "C" fn reboot(_: i32) {
    end();
    sysreset(RebootMode::RB_AUTOBOOT);
}

extern "C" fn poweroff(_: i32) {
    end();
    sysreset(RebootMode::RB_POWER_OFF);
}

fn main() {
    if process::id() != 1 {
        log!(Color::Red, "[ ERROR ] must be run as PID 1");
        halt!();
    }

    let _boot_handle = mount_or_halt(1, "/boot", "vfat");
    let _data_handle = mount_or_halt(4, "/data", "ext4");
    let _proc_handle = Mount::builder()
        .fstype("proc")
        .mount("proc", "/proc")
        .expect("can't mount /proc procfs");
    let _tmp_handle = Mount::builder()
        .fstype("tmpfs")
        .mount("tmpfs", "/tmp")
        .expect("can't mount /tmp tmpfs");
    let _run_handle = Mount::builder()
        .fstype("tmpfs")
        .mount("tmpfs", "/run")
        .expect("can't mount /run tmpfs");

    match start() {
        Ok(_) => {}
        Err(e) => log!(Color::Red, "[ ERROR ] {}", e),
    }

    let reboot_action = SigAction::new(
        SigHandler::Handler(reboot),
        SaFlags::empty(),
        SigSet::from(Sig::SIGUSR1),
    );

    let shutdown_action = SigAction::new(
        SigHandler::Handler(poweroff),
        SaFlags::empty(),
        SigSet::from(Sig::SIGUSR2),
    );

    unsafe {
        match nix::sys::signal::sigaction(Sig::SIGUSR1, &reboot_action) {
            Ok(_) => {}
            Err(e) => log!(
                Color::Red,
                "[ ERROR ] can't subscribe to SIGUSR1: {}",
                e.desc()
            ),
        }
    }

    unsafe {
        match nix::sys::signal::sigaction(Sig::SIGUSR2, &shutdown_action) {
            Ok(_) => {}
            Err(e) => log!(
                Color::Red,
                "[ ERROR ] can't subscribe to SIGUSR2: {}",
                e.desc()
            ),
        }
    }

    halt!();
}

fn sysreset(reboot_mode: RebootMode) {
    nix::unistd::sync();

    log!(Color::Yellow, "[ INFO  ] send final SIGTERM");
    for process in System::new_all().processes().values() {
        process.kill_with(Signal::Term);
    }

    thread::sleep(Duration::from_secs(3));

    log!(Color::Yellow, "[ INFO  ] send final SIGKILL");
    for process in System::new_all().processes().values() {
        process.kill_with(Signal::Kill);
    }

    let Err(e) = nix::sys::reboot::reboot(reboot_mode);
    log!(
        Color::Red,
        "[ ERROR ] can't reboot (mode: {:?}): {}",
        reboot_mode,
        e
    );
    halt!();
}
