use anyhow::bail;
use std::fs;
use std::io::{self, Write};
use std::os::fd::AsFd;
use std::process::{Command, Stdio};
use std::thread;
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
        cmd.stderr(Stdio::null())
            .stdin(Stdio::null())
            .stdout(Stdio::null());

        match cmd.spawn() {
            Ok(_) => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
                writeln!(&mut stdout, "[  OK   ] Starting {}", service_name)?;

                stdout.reset()?;
                stdout.flush()?;

                cmd.stderr(io::stderr().as_fd().try_clone_to_owned()?);
                cmd.stdin(io::stdin().as_fd().try_clone_to_owned()?);
                cmd.stdout(io::stdout().as_fd().try_clone_to_owned()?);
            }
            Err(e) => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
                writeln!(&mut stdout, "[ ERROR ] Starting {}: {}", service_name, e)?;

                stdout.reset()?;
                stdout.flush()?;
            }
        }
    }

    Ok(())
}

fn main() {
    match start() {
        Ok(_) => {}
        Err(e) => {
            let mut stdout = StandardStream::stdout(ColorChoice::Always);

            match stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red))) {
                Ok(_) => match writeln!(&mut stdout, "[ ERROR ] {}", e) {
                    Ok(_) => {}
                    Err(_) => println!("[ ERROR ] {}", e),
                },
                Err(_) => {
                    println!("[ ERROR ] {}", e);
                }
            }
        }
    }

    loop {
        thread::yield_now();
    }
}
