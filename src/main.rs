use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn print_usage(program: &str, opts: &getopts::Options) {
    let brief = format!(
        r#"Echo benchmark.

Usage:
  {program} [ -a <address> ] [ -l <length> ] [ -c <number> ] [ -t <duration> ]
  {program} (-h | --help)
  {program} --version"#,
        program = program
    );
    print!("{}", opts.usage(&brief));
}

fn main() {
    let args: Vec<_> = env::args().collect();
    let program = args[0].clone();

    let mut opts = getopts::Options::new();
    opts.optflag("h", "help", "Print this help.");
    opts.optopt(
        "a",
        "address",
        "Target echo server address. Default: 127.0.0.1:12345",
        "<address>",
    );
    opts.optopt(
        "l",
        "length",
        "Test message length. Default: 512",
        "<length>",
    );
    opts.optopt(
        "t",
        "duration",
        "Test duration in seconds. Default: 60",
        "<duration>",
    );
    opts.optopt(
        "c",
        "number",
        "Test connection number. Default: 50",
        "<number>",
    );

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => {
            eprintln!("{}", f.to_string());
            print_usage(&program, &opts);
            return;
        }
    };

    if matches.opt_present("h") {
        print_usage(&program, &opts);
        return;
    }

    let length = matches
        .opt_str("length")
        .unwrap_or_default()
        .parse::<usize>()
        .unwrap_or(512);
    let duration = matches
        .opt_str("duration")
        .unwrap_or_default()
        .parse::<u64>()
        .unwrap_or(60);
    let number = matches
        .opt_str("number")
        .unwrap_or_default()
        .parse::<u32>()
        .unwrap_or(50);
    let address = matches
        .opt_str("address")
        .unwrap_or_else(|| "127.0.0.1:8901".to_string());

    // max open file
    let mut nofile_rlimit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    unsafe {
        if 0 != libc::getrlimit(libc::RLIMIT_NOFILE, &mut nofile_rlimit) {
            panic!("getrlimit failed");
        }
        if nofile_rlimit.rlim_max < number as u64 + 3 {
            panic!(
                "the hard limit of this process is only {}",
                nofile_rlimit.rlim_max
            )
        }
        nofile_rlimit.rlim_cur = nofile_rlimit.rlim_max.min(number as u64 + 3);
        if 0 != libc::setrlimit(libc::RLIMIT_NOFILE, &nofile_rlimit) {
            panic!("setrlimit failed");
        }
    }

    let stop = Arc::new(AtomicBool::new(false));
    let control = Arc::downgrade(&stop);

    let group: Vec<_> = (0..number)
        .map(|i| (i, address.clone(), stop.clone(), length))
        .map(|(i, address, stop, length)| {
            thread::spawn(move || {
                let mut count = 0;
                let mut out_buf: Vec<u8> = vec![0; length];
                out_buf[length - 1] = b'\n';
                let mut in_buf: Vec<u8> = vec![0; length];
                let mut stream = TcpStream::connect(address).unwrap();

                while !stop.load(Ordering::Relaxed) {
                    if let Err(e) = stream.write_all(&out_buf) {
                        println!("thread {i} write error: {e}");
                        return (count, 1);
                    }

                    if let Err(e) = stream.read_exact(&mut in_buf) {
                        println!("thread {i} read error: {e}");
                        return (count, 1);
                    };
                    count += 1;
                }
                (count, 0)
            })
        })
        .collect();

    thread::sleep(Duration::from_secs(duration));

    control.upgrade().unwrap().store(true, Ordering::Relaxed);

    let (n_req, n_error) = group
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .reduce(|(a, b), (c, d)| (a + c, b + d))
        .unwrap();

    println!(
        "Benchmarking: {address}
{number} clients, running {length} bytes, {duration} sec.

Error: {n_error}
Speed: {} request/sec",
        n_req / duration
    );
}
