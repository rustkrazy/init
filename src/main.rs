use anyhow::bail;
use std::fs;
use std::io::Write;
use std::process::{self, Command, ExitCode};
use std::thread;
use std::time::Duration;
use sys_mount::{Mount, Unmount, UnmountFlags};
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

        match Command::new(service.path()).spawn() {
            Ok(_) => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
                write!(&mut stdout, "[  OK   ] Starting {}", service_name)?;

                stdout.reset()?;
                writeln!(&mut stdout)?;
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

fn main() -> ExitCode {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);

    let _mount;

    match Mount::builder()
        .fstype("vfat")
        .mount("/dev/disk/by-partuuid/00000000-01", "/boot")
    {
        Ok(v) => _mount = v.into_unmount_drop(UnmountFlags::DETACH),
        Err(e) => {
            match stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red))) {
                Ok(_) => match writeln!(&mut stdout, "[ ERROR ] Can't mount /boot: {}", e) {
                    Ok(_) => {}
                    Err(_) => println!("[ ERROR ] Can't mount /boot: {}", e),
                },
                Err(_) => {
                    println!("[ ERROR ] Can't mount /boot: {}", e);
                }
            }

            loop {
                thread::sleep(Duration::MAX);
            }
        }
    };

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
