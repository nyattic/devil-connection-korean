use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dc_installer::{
    Event, InstallConfig, Level, Reporter, TranslationSource, detect_game_dirs, game, install,
    locate_asar, restore,
};

#[derive(Parser)]
#[command(name = "dc-patcher", about = "데빌 커넥션 한글패치 프로그램", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Detect,

    Info {
        #[command(flatten)]
        target: Target,
    },

    Install {
        #[command(flatten)]
        target: Target,

        #[arg(long, value_name = "경로")]
        data_dir: Option<PathBuf>,

        #[arg(long)]
        keep_work_dir: bool,
    },

    Restore {
        #[command(flatten)]
        target: Target,
    },
}

#[derive(clap::Args)]
struct Target {
    #[arg(long, value_name = "경로")]
    game_dir: Option<PathBuf>,

    #[arg(long, value_name = "경로", conflicts_with = "game_dir")]
    asar: Option<PathBuf>,
}

impl Target {
    fn resolve(&self) -> dc_installer::Result<PathBuf> {
        if let Some(asar) = &self.asar {
            return locate_asar(asar);
        }
        if let Some(dir) = &self.game_dir {
            return locate_asar(dir);
        }
        locate_asar(&game::detect_game_dir()?)
    }
}

struct ConsoleReporter;

impl Reporter for ConsoleReporter {
    fn report(&self, event: Event) {
        match event {
            Event::Step {
                index,
                total,
                message,
            } => println!("\n[{index}/{total}] {message}"),
            Event::Message { level, text } => {
                let prefix = match level {
                    Level::Info => "  ",
                    Level::Success => "  + ",
                    Level::Warning => "  ! ",
                    Level::Error => "  x ",
                };
                println!("{prefix}{text}");
            }
            Event::Progress { label, done, total } => {
                println!("  … {label} ({done}/{total})");
            }
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\n오류: {e}");
            let mut source = std::error::Error::source(&*e);
            while let Some(inner) = source {
                eprintln!("  원인: {inner}");
                source = inner.source();
            }
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> std::result::Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Detect => {
            let dirs = detect_game_dirs();
            if dirs.is_empty() {
                println!("게임을 자동으로 찾지 못했습니다. --game-dir로 직접 지정해주세요.");
                return Ok(());
            }
            for dir in dirs {
                match locate_asar(&dir) {
                    Ok(asar) => println!("{}\n  app.asar: {}", dir.display(), asar.display()),
                    Err(e) => println!("{}\n  {}", dir.display(), e),
                }
            }
        }

        Command::Info { target } => {
            let asar = target.resolve()?;
            let archive = dc_asar::AsarArchive::open(&asar)?;
            let entries = archive.entries();
            let files = entries
                .iter()
                .filter(|e| matches!(e.kind, dc_asar::EntryKind::File { .. }))
                .count();

            println!("경로:       {}", asar.display());
            println!("크기:       {}바이트", std::fs::metadata(&asar)?.len());
            println!("데이터 시작: {}", archive.data_offset());
            println!("항목 수:     {} (파일 {files}개)", entries.len());
            println!(
                "백업:       {}",
                if backup_path(&asar).is_file() {
                    "있음"
                } else {
                    "없음"
                }
            );
            match archive.validate() {
                Ok(()) => println!("헤더 검증:   통과"),
                Err(e) => println!("헤더 검증:   실패 - {e}"),
            }
        }

        Command::Install {
            target,
            data_dir,
            keep_work_dir,
        } => {
            let asar = target.resolve()?;
            let data_dir = match data_dir.or_else(dc_installer::find_data_dir) {
                Some(dir) => dir,
                None => {
                    return Err(
                        "번역 데이터 폴더를 찾지 못했습니다. --data-dir로 직접 지정해주세요."
                            .into(),
                    );
                }
            };

            println!("게임:        {}", asar.display());
            println!("번역 데이터: {}", data_dir.display());

            let report = install(
                &InstallConfig {
                    asar_path: asar,
                    source: TranslationSource::Directory(data_dir),
                    keep_work_dir,
                },
                &ConsoleReporter,
            )?;

            println!("\n한글패치가 완료되었습니다.");
            println!("  번역 파일:  {}개", report.copied_files);
            println!("  검증 완료:  {}개", report.verified_files);
            println!("  백업 위치:  {}", report.backup_path.display());
            println!("\n원래대로 되돌리려면: dc-patcher restore");
        }

        Command::Restore { target } => {
            let asar = target.resolve()?;
            restore(&asar, &ConsoleReporter)?;
        }
    }

    Ok(())
}

fn backup_path(asar: &Path) -> PathBuf {
    let mut name = asar.file_name().unwrap_or_default().to_os_string();
    name.push(".backup");
    asar.with_file_name(name)
}
