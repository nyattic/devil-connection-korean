# 개발 안내

`PRIVATE_DISTRIBUTION_PLAN.md`의 **1단계: 기술 검증용 프로토타입** 구현이다.
기존 `main.py` + 파이썬 `asar` 패키지가 하던 일을 Rust로 옮기고, 계획서 2.2절이 지적한
설치 안전성 문제를 해결했다.

## 구성

| 경로 | 역할 |
| --- | --- |
| `src/` | `devil-connection-korean` 루트 패키지. `dc-patcher-gui` 그래픽 설치기 (egui) |
| `crates/dc-asar` | Electron ASAR 아카이브 읽기/쓰기 |
| `crates/dc-installer` | 게임 경로 탐색, 트랜잭션 설치, `dc-patcher` CLI |
| `assets/` | 설치기 화면에 쓰는 Pretendard JP 서체 |
| `data/`, `tyrano/` | 게임에 덮어쓸 번역 데이터 |

설치 로직은 전부 `dc-installer`에 있고 CLI와 GUI가 함께 쓴다. 화면에 보여줄 문구는
`progress::Event`로 넘어가므로 라이브러리는 표준 출력에 직접 쓰지 않는다.

## 사용법

```sh
cargo build --release

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

계획서 2.2절의 *"기존 코드는 새 `app.asar` 생성 전에 원본을 제거하므로, 재압축 실패 시
게임 파일이 불완전해질 수 있다"*에 대응한다.

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
cargo test        # 49개 (단위 + 통합)
cargo clippy --all-targets
cargo fmt --all -- --check
```

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

## 아직 구현하지 않은 것

계획서 2~8단계에 해당한다.

- 개인화 패키지 빌더, 라이선스/패키지 형식, 서명·암호화 (`dc-core`)
- NAS 활성화 API와 관리자 CLI
- 기기 키 생성 및 OS 보안 저장소 보관
- 등록·인증·다운로드 흐름
- 플랫폼별 배포 패키징(코드 서명, macOS 앱 번들, Windows 아이콘)

현재 설치기는 `--data-dir`의 평문 번역 데이터를 그대로 읽는다.
계획서 4장(권리·라이선스 확정)이 정리되기 전까지 기기 귀속형 배포는 운영에 투입하지 않는다.
