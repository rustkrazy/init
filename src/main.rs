use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{self, ChildStderr, ChildStdout, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::bail;
use sys_mount::{Mount, Unmount, UnmountDrop, UnmountFlags};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

fn start() -> anyhow::Result<()> {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);

    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
    writeln!(&mut stdout, "Starting rustkrazy")?;

    for service in fs::read_dir("/bin")? {
        let service = service?;
        let service_name = match service.file_name().into_string() {
            Ok(v) => v,
            Err(_) => bail!("[ ERROR ] invalid unicode in file name"),
        };

        if service_name == "init" {
            continue;
        }

        let mut cmd = Command::new(service.path());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        match cmd.spawn() {
            Ok(child) => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
                write!(&mut stdout, "[  OK   ] Starting {}", service_name)?;

                stdout.reset()?;
                writeln!(&mut stdout)?;

                let service_name2 = service_name.clone();
                thread::spawn(move || {
                    log_out(child.stdout.expect("no child stdout"), service_name2)
                        .expect("logging stdout failed");
                });

                thread::spawn(move || {
                    log_err(child.stderr.expect("no child stderr"), service_name)
                        .expect("logging stderr failed");
                });
            }
            Err(e) => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
                write!(&mut stdout, "[ ERROR ] Starting {}: {}", service_name, e)?;

                stdout.reset()?;
                writeln!(&mut stdout)?;
            }
        }
    }

    Ok(())
}

fn log_out(pipe: ChildStdout, service_name: String) -> anyhow::Result<()> {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    let mut file = File::create(Path::new("/data").join(service_name.clone() + ".log"))?;
    let mut r = BufReader::new(pipe);

    loop {
        let mut buf = String::new();
        r.read_line(&mut buf)?;

        if !buf.is_empty() {
            let timestamp = humantime::format_rfc3339_seconds(SystemTime::now());
            let buf = format!("[{} {}] {}", timestamp, service_name, buf);

            stdout.set_color(ColorSpec::new().set_fg(Some(Color::White)))?;
            write!(&mut stdout, "{}", buf)?;

            file.write_all(buf.as_bytes())?;
        }
    }
}

fn log_err(pipe: ChildStderr, service_name: String) -> anyhow::Result<()> {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    let mut file = File::create(Path::new("/data").join(service_name.clone() + ".err"))?;
    let mut r = BufReader::new(pipe);

    loop {
        let mut buf = String::new();
        r.read_line(&mut buf)?;

        if !buf.is_empty() {
            let timestamp = humantime::format_rfc3339_seconds(SystemTime::now());
            let buf = format!("[{} {}] {}", timestamp, service_name, buf);

            stdout.set_color(ColorSpec::new().set_fg(Some(Color::White)))?;
            write!(&mut stdout, "{}", buf)?;

            file.write_all(buf.as_bytes())?;
        }
    }
}

fn mount_or_halt(part_id: u8, mount_point: &str, fs: &str) -> UnmountDrop<Mount> {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);

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
                match stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red))) {
                    Ok(_) => {
                        match writeln!(&mut stdout, "[ ERROR ] Can't mount {}: {}", mount_point, e)
                        {
                            Ok(_) => {}
                            Err(_) => println!("[ ERROR ] Can't mount {}: {}", mount_point, e),
                        }
                    }
                    Err(_) => println!("[ ERROR ] Can't mount {}: {}", mount_point, e),
                }
            } else {
                match stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red))) {
                    Ok(_) => match writeln!(
                        &mut stdout,
                        "[ ERROR ] Can't mount {}: Unknown error (this shouldn't happen)",
                        mount_point
                    ) {
                        Ok(_) => {}
                        Err(_) => println!(
                            "[ ERROR ] Can't mount {}: Unknown error (this shouldn't happen)",
                            mount_point
                        ),
                    },
                    Err(_) => {
                        println!(
                            "[ ERROR ] Can't mount {}: Unknown error (this shouldn't happen)",
                            mount_point
                        )
                    }
                }
            }

            loop {
                thread::sleep(Duration::MAX);
            }
        }
        Some(handle) => handle,
    }
}

fn main() -> ExitCode {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);

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
        match stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red))) {
            Ok(_) => match writeln!(&mut stdout, "Must be run as PID 1") {
                Ok(_) => {}
                Err(_) => println!("Must be run as PID 1"),
            },
            Err(_) => {
                println!("Must be run as PID 1");
            }
        }

        loop {
            thread::sleep(Duration::MAX);
        }
    }

    match start() {
        Ok(_) => {}
        Err(e) => match stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red))) {
            Ok(_) => match writeln!(&mut stdout, "[ ERROR ] {}", e) {
                Ok(_) => {}
                Err(_) => println!("[ ERROR ] {}", e),
            },
            Err(_) => {
                println!("[ ERROR ] {}", e);
            }
        },
    }

    loop {
        thread::sleep(Duration::MAX);
    }
}
