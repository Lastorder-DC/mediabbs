# MediaBBS

**MediaBBS**는 MediaWiki 계정으로 로그인하는 게시판입니다. 위키 사용자는 별도 가입 없이
위키 계정(OAuth 2.0)으로 글을 쓰고, 위키 관리자는 그대로 게시판 관리자가 됩니다.

[리브레 위키](https://librewiki.net)의 [LibreBBS](https://github.com/librewiki/librebbs) /
[librebbs-client](https://github.com/librewiki/librebbs-client)를 포크해,
리브레 위키에 고정돼 있던 설정을 모두 설정값으로 바꾸고 **아무 MediaWiki에나 붙일 수 있게** 만든
범용 버전입니다.

이 저장소는 **백엔드(API 서버)** 입니다. 웹 화면은
[mediabbs-client](https://github.com/Lastorder-DC/mediabbs-client)에 있습니다.

## 기능

- MediaWiki OAuth 2.0 로그인 (Extension:OAuth), 비로그인(IP) 글쓰기
- 위키 그룹 기반 관리자 (`ADMIN_GROUPS`): 주제 숨기기·잠그기·종료·상단 고정, 의견 숨기기
- 위키 차단 연동: 차단된 계정과 차단된 IP(대역 포함)는 글을 쓸 수 없음
- 이미지 업로드: S3, S3 호환 스토리지(MinIO, Cloudflare R2 등), 로컬 디스크
- 위키 정보 자동 탐지(사이트 이름, 로고, 문서 주소 형식, 이름공간) — 위키 언어와 무관
- 게시판 관리 CLI, 내장 DB 마이그레이션, Docker 이미지

## 구성

```
 브라우저 ──▶ mediabbs-client (정적 파일)
    │
    ├──▶ mediabbs (이 저장소, REST API) ──▶ MySQL / MariaDB
    │         │                        └──▶ S3 또는 로컬 디스크 (업로드)
    │         └──▶ MediaWiki rest.php / api.php (토큰 교환, 사용자 정보, 차단 확인)
    │
    └──▶ MediaWiki (로그인·권한 승인 화면, 최근 바뀜·공지 읽기)
```

## 빠르게 써 보기

Docker만 있으면 MediaWiki까지 포함한 전체 구성을 로컬에서 띄울 수 있습니다
(`mediabbs-client`를 이 저장소 옆에 함께 받아 두어야 합니다).

```sh
git clone https://github.com/Lastorder-DC/mediabbs.git
git clone https://github.com/Lastorder-DC/mediabbs-client.git
cd mediabbs/dev
docker compose up -d --build
```

http://127.0.0.1:18090 에 접속해 `Alice` / `mediabbs-dev-user` 로 로그인해 보세요.
자세한 내용은 [dev/README.md](dev/README.md)를 보세요.

## 문서

| 문서 | 내용 |
|---|---|
| [설치 안내](docs/installation.md) | MediaWiki 준비부터 백엔드·클라이언트 배포, 프록시 구성까지 |
| [설정 레퍼런스](docs/configuration.md) | 모든 환경 변수와 형식 |
| [운영 안내](docs/administration.md) | 게시판 관리, 관리자 기능, 업그레이드, LibreBBS에서 옮겨 오기 |
| [API](docs/api.md) | REST API 엔드포인트 |
| [문제 해결](docs/troubleshooting.md) | 자주 겪는 문제와 해결 방법 |

## 개발

```sh
cargo build                 # Rust 1.94+, libmysqlclient-dev, libssl-dev, pkg-config 필요
cargo test --lib            # 단위 테스트 (DB가 필요한 테스트는 DATABASE_URL 설정 시 실행)
cd dev && docker compose up -d --build && cd e2e && npm install && node oauth-e2e.js
```

## 라이선스

MIT. 원본 LibreBBS의 저작권 표기(Copyright (c) 2021 Librewiki Developers)를 따릅니다.
[LICENSE](LICENSE)를 보세요.
