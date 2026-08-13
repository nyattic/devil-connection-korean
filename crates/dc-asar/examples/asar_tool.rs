use std::process::ExitCode;

use dc_asar::{AsarArchive, EntryKind, PackOptions, create_archive};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let result = match args.first().map(String::as_str) {
        Some("pack") if args.len() >= 3 => pack(&args),
        Some("extract") if args.len() >= 3 => extract(&args[1], &args[2]),
        Some("list") if args.len() >= 2 => list(&args[1]),
        _ => {
            eprintln!("사용법: asar_tool <pack|extract|list> ...");
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("오류: {e}");
            ExitCode::FAILURE
        }
    }
}

fn pack(args: &[String]) -> dc_asar::Result<()> {
    let mut options = PackOptions {
        unpack: Vec::new(),
        ..PackOptions::default()
    };

    let mut rest = args[3..].iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--unpack" => {
                if let Some(pattern) = rest.next() {
                    options.unpack.push(pattern.clone());
                }
            }
            other => eprintln!("알 수 없는 옵션 무시: {other}"),
        }
    }

    let stats = create_archive(&args[1], &args[2], &options)?;
    println!(
        "파일 {} (unpacked {}), 폴더 {}, 링크 {}, {}바이트",
        stats.files, stats.unpacked_files, stats.directories, stats.links, stats.archive_bytes
    );
    Ok(())
}

fn extract(archive: &str, dest: &str) -> dc_asar::Result<()> {
    let mut opened = AsarArchive::open(archive)?;
    opened.validate()?;
    let stats = opened.extract_to(dest)?;
    println!(
        "파일 {} (unpacked {}), 폴더 {}, 링크 {}, {}바이트",
        stats.files, stats.unpacked_files, stats.directories, stats.links, stats.bytes
    );
    Ok(())
}

fn list(archive: &str) -> dc_asar::Result<()> {
    let opened = AsarArchive::open(archive)?;
    opened.validate()?;
    for entry in opened.entries() {
        match entry.kind {
            EntryKind::Directory => println!("d              {}", entry.path),
            EntryKind::Link { target } => println!("l              {} -> {target}", entry.path),
            EntryKind::File {
                size,
                offset,
                unpacked,
                ..
            } => {
                let location = if unpacked {
                    "unpacked".to_string()
                } else {
                    offset.map(|o| o.to_string()).unwrap_or_default()
                };
                println!("f {size:>10} @{location:<10} {}", entry.path);
            }
        }
    }
    Ok(())
}
