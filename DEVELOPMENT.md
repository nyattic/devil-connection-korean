# 개발 안내

기존 `main.py` + 파이썬 `asar` 패키지가 하던 일을 Rust로 옮기고, 파이썬 설치기가 안고
있던 설치 안전성 문제를 해결했다. 배포용 빌드는 번역 데이터를 실행 파일 안에 포함하므로
사용자가 받는 것은 파일 하나다.

## 구성

| 경로 | 역할 |
| --- | --- |
| `src/` | `devil-connection-korean` 루트 패키지. `dc-patcher-gui` 그래픽 설치기 (egui) |
| `crates/dc-asar` | Electron ASAR 아카이브 읽기/쓰기 |
| `crates/dc-installer` | 게임 경로 탐색, 트랜잭션 설치, `dc-patcher` CLI |
| `build.rs` | `embed-data` 기능을 켜면 번역 데이터를 아카이브로 묶어 `OUT_DIR`에 둔다 |
| `src/embedded.rs` | 그 아카이브를 `include_bytes!`로 실행 파일에 싣는다 |
| `assets/` | 설치기 화면에 쓰는 Pretendard JP 서체 |
| `data/`, `tyrano/` | 게임에 덮어쓸 번역 데이터 |

설치 로직은 전부 `dc-installer`에 있고 CLI와 GUI가 함께 쓴다. 화면에 보여줄 문구는
`progress::Event`로 넘어가므로 라이브러리는 표준 출력에 직접 쓰지 않는다.

## 사용법

```sh
# 개발용. 번역 데이터를 포함하지 않으므로 빌드가 빠르다.
cargo build --release

# 배포용. 번역 데이터를 실행 파일에 포함한다.
cargo build --release --features embed-data

# 그래픽 설치기
./target/release/dc-patcher-gui

# 설치된 게임 찾기
./target/release/dc-patcher detect

# app.asar 정보 확인
./target/release/dc-patcher info --game-dir <게임폴더>

# 설치 (--data-dir 생략 시 실행 파일 주변에서 data/, tyrano/를 찾는다)
./target/release/dc-patcher install

# 원본 복구
./target/release/dc-patcher restore
```

`--game-dir`와 `--asar`를 모두 생략하면 Steam 라이브러리에서 자동으로 찾는다.
Windows에서는 드라이브 문자 추정에 더해 `libraryfolders.vdf`에 등록된 추가 라이브러리도 본다.

## 기존 파이썬 설치기와 달라진 점

파이썬 설치기는 새 `app.asar`을 만들기 전에 원본을 지웠기 때문에, 재압축이 실패하면
게임 파일이 불완전한 상태로 남았다. 그 문제에 대응한다.

| 항목 | 파이썬 설치기 | Rust 설치기 |
| --- | --- | --- |
| 작업 위치 | `resources/app` (Electron이 읽을 수 있는 경로) | `resources/.dcpatch-work-N/` |
| 원본 제거 시점 | 재압축 **전** | 검증까지 끝난 **후** |
| 재압축 실패 시 | 게임 실행 불가 | 원본 그대로 유지 |
| 교체 방식 | 삭제 후 생성 | 검증 후 `rename` |
| 설치 후 검증 | 없음 | 번역 파일 755개 SHA-256 대조 |
| 재설치 기준 | 현재 `app.asar` (패치본 위에 덧씌움) | `app.asar.backup` (항상 원본에서 시작) |
| 사전 검사 | 없음 | 쓰기 권한, 디스크 여유 공간, 헤더 유효성, 번역 데이터 완전성 |
| 복구 명령 | 없음 | `dc-patcher restore` |

## 백업 파일

```
app.asar                  패치된 아카이브
app.asar.backup           원본 (최초 설치 시 생성, 이후 절대 덮어쓰지 않음)
app.asar.unpacked/        *.node 등 아카이브 밖에 두는 파일
app.asar.backup.unpacked/ 위 폴더의 원본
```

`app.asar.backup`은 한 번 만들어지면 갱신하지 않는다. 재설치할 때도 이 백업에서 시작하므로
몇 번을 실행해도 결과가 같다.

## 화면

단일 컬럼 가운데 정렬이다. 사용자가 하는 일이 "폴더 두 개를 확인하고 한 번 누르고
기다리는 것"뿐이라 화면을 나눌 이유가 없다.

- 제목은 한국어(`데빌 커넥션 한글패치`)가 위, 원제(`でびるコネクショん`)가 부제다.
  한국어 사용자를 위한 도구이므로 아는 이름을 먼저 두고, 원제는 "이 게임이 맞는지"
  확인하는 용도로 내렸다.
- 강조색 `#8A3557`은 게임 메뉴 탭(`data/image/menu_syoukan.png`)의 자두색을 밝은
  배경에 맞게 조정한 값이다. 색은 이 하나만 쓴다.
- 본문 글꼴은 Pretendard JP다. 한글과 가나를 한 글꼴로 그릴 수 있어 제목과 UI에
  서로 다른 글꼴이 섞이지 않는다.
- 기록 본문만 왼쪽 정렬이다. 줄 길이가 제각각이라 가운데로 모으면 읽기 어렵다.

진행 상황은 `dc_installer::STEPS`에서 단계 이름을 가져와 표시하므로, 설치 로직이
단계를 바꾸면 화면도 함께 바뀐다.

## 검증

```sh
cargo test                                    # 50개 (단위 + 통합)
cargo clippy --all-targets
cargo clippy --all-targets --features embed-data
cargo fmt --all -- --check
```

번역 데이터 출처는 `TranslationSource`로 나뉜다. `Directory`는 `data/`, `tyrano/`를
담은 폴더를, `Embedded`는 실행 파일에 포함된 아카이브를 읽는다. 두 경로가 같은 결과를
내는지는 통합 테스트에서 생성된 `app.asar`을 바이트 단위로 비교해 확인한다.

Node.js `@electron/asar`와의 호환성은 다음을 확인했다.

- Rust로 만든 아카이브를 Node가 동일하게 해제
- Node가 만든 아카이브를 Rust가 동일하게 해제
- `unpack` 글롭 판정 결과 일치
- `integrity` 블록이 Node 구현과 완전히 일치
- Node `asar` 런타임(`extractFile`, `getRawHeader`, `listPackage`)이 Rust 아카이브를 정상 처리

실물 규모(137MB, 파일 837개) 설치는 약 1.2초가 걸리며, 복구 후 원본과 바이트 단위로 일치한다.

## 개발용 도구

```sh
cargo run -p dc-asar --example asar_tool -- pack <폴더> <출력.asar> --unpack '*.node'
cargo run -p dc-asar --example asar_tool -- extract <입력.asar> <폴더>
cargo run -p dc-asar --example asar_tool -- list <입력.asar>
```

## 남은 작업

- 패치된 게임을 실제로 실행해 확인. 지금까지 확인한 것은 복구 후 원본과 바이트 단위로
  일치한다는 점까지다.
- Windows에서의 설치 흐름 확인. 개발과 검증은 macOS에서만 했다.
- 3개 플랫폼 빌드와 릴리스 자동화. `.github/` 자체가 아직 없다.
- 배포 패키징. Windows 아이콘, macOS 앱 번들과 공증.

`dc-patcher` CLI는 번역 데이터를 포함하지 않는다. 실행 파일 두 개에 같은 137MB를
중복해서 넣을 이유가 없어서다. CLI로 설치할 때는 `--data-dir`로 폴더를 지정한다.
