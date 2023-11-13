use std::fs::{self, DirEntry, File};
use std::io::{BufRead, BufReader, Seek, Write};
use std::path::Path;
use std::process::{self, ChildStderr, ChildStdout, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::bail;
use sys_mount::{Mount, Unmount, UnmountDrop, UnmountFlags};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

const SERVICE_RESTART_INTERVAL: Duration = Duration::from_secs(30);

macro_rules! log {
    ($col:expr, $($tts:tt)*) => {
        {
            let mut stdout = StandardStream::stdout(ColorChoice::Always);

            match stdout.set_color(ColorSpec::new().set_fg(Some($col))) {
                Ok(_) => match write!(&mut stdout, $($tts)*) {
                    Ok(_) => {
                        stdout.reset().ok();
                        match writeln!(&mut stdout) {
                            Ok(_) => {}
                            Err(_) => println!(),
                        }
                    }
                    Err(_) => println!($($tts)*),
                }
                Err(_) => println!($($tts)*),
            }
        }
    };
}

macro_rules! log_raw {
    ($col:expr, $($tts:tt)*) => {
        {
            let mut stdout = StandardStream::stdout(ColorChoice::Always);

            match stdout.set_color(ColorSpec::new().set_fg(Some($col))) {
                Ok(_) => match write!(&mut stdout, $($tts)*) {
                    Ok(_) => {}
                    Err(_) => print!($($tts)*),
                }
                Err(_) => print!($($tts)*),
            }
        }
    };
}

macro_rules! halt {
    () => {
        thread::park();

        // Just in case. Still better than panicking.
        loop {
            thread::sleep(Duration::MAX);
        }
    };
}

fn start() -> anyhow::Result<()> {
    log!(Color::Yellow, "Starting rustkrazy");

    for service in fs::read_dir("/bin")? {
        let service = service?;
        let service_name = match service.file_name().into_string() {
            Ok(v) => v,
            Err(_) => bail!("[ ERROR ] invalid unicode in file name"),
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

fn supervise(service: DirEntry, service_name: String) -> anyhow::Result<()> {
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

fn log_out(pipe: ChildStdout, service_name: String) -> anyhow::Result<()> {
    let mut file = File::create(Path::new("/tmp").join(service_name.clone() + ".log"))?;
    let mut r = BufReader::new(pipe);

    loop {
        let mut buf = String::new();
        r.read_line(&mut buf)?;

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

fn log_err(pipe: ChildStderr, service_name: String) -> anyhow::Result<()> {
    let mut file = File::create(Path::new("/tmp").join(service_name.clone() + ".err"))?;
    let mut r = BufReader::new(pipe);

    loop {
        let mut buf = String::new();
        r.read_line(&mut buf)?;

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

fn main() -> ExitCode {
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

    if process::id() != 1 {
        log!(Color::Red, "[ ERROR ] must be run as PID 1");
        halt!();
    }

    match start() {
        Ok(_) => {}
        Err(e) => log!(Color::Red, "[ ERROR ] {}", e),
    }

    halt!();
}
