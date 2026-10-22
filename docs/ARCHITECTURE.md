# YAOPLM — архитектура

> Версия 0.1 от 2026-10-22. Обоснование выбора стека — [research/04-stack.md](research/04-stack.md) и [DECISIONS.md](DECISIONS.md).

## 1. Общая схема

```
                ┌───────────────────────── web/ (React + TS SPA) ─────────────────────────┐
                │  features: library · game · devices · jobs · settings                    │
                │  src/api/transport.ts:  TauriTransport | HttpTransport | MockTransport   │
                └──────────────┬──────────────────────────────┬────────────────────────────┘
                     invoke + Channel                 HTTP /api/rpc + WS /api/events
                ┌──────────────▼─────────────┐   ┌────────────▼─────────────┐   ┌───────────────┐
                │ apps/desktop (Tauri 2)     │   │ apps/server (axum)       │   │ apps/cli      │
                │ тонкий адаптер             │   │ тонкий адаптер + SPA     │   │ (clap)        │
                └──────────────┬─────────────┘   └────────────┬─────────────┘   └──────┬────────┘
                               └──────────────┬───────────────┴────────────────────────┘
                                   crates/yaoplm-api  (Request / Response / JobEvent, dispatch, JobManager)
                                              │
     ┌──────────────┬───────────────┬─────────┴─────┬────────────────┬──────────────┬──────────────┐
  yaoplm-core   yaoplm-formats   yaoplm-hash    yaoplm-device     yaoplm-net     yaoplm-hdd (later)
  модель,       ISO9660, CNF,    один проход:   диски, ФС,        источники      APA/PFS
  сканер,       ul.cfg, CFG,     CRC32/SHA1/    копирование,      обложек и
  задания,      ZSO, CUE, VMC,   MD5, DAT       нарезка,          метаданных,
  SQLite-кэш    ART, CHT, APPS   matcher        фрагментация      ресайз, SMB
```

**Главное правило:** крейты ядра (`yaoplm-core`, `-formats`, `-hash`, `-device`) **не зависят от tauri, axum и tokio**. В CI это проверяет `cargo tree -i tauri -p yaoplm-core` (и аналогичные команды для axum и tokio): ожидаем пустой результат. Ядро синхронное: `std::fs` плюс потоки (rayon или std::thread). Асинхронность есть только на краях (`yaoplm-net`, `apps/server`, `apps/desktop`).

## 2. Стек

Версии проверены 2026-10-22 (research/04). Перед добавлением зависимости сверяйте актуальную версию и лицензию.

| Область | Выбор | Версия | Комментарий |
|---|---|---|---|
| Язык ядра | Rust stable (MSVC на Windows) | ≥1.90, фиксируется в `rust-toolchain.toml` | |
| Десктоп | Tauri | 2.12.x | Tauri 3 пока в alpha, не используем |
| Сервер | axum | 0.8.x | tokio только здесь и в desktop/net |
| CLI | clap (derive), indicatif | актуальные | |
| Типы для TS | ts-rs | 12 | **не** tauri-specta (он ещё RC) |
| Фронтенд | React 19 + TypeScript + Vite | актуальные | менеджер пакетов pnpm через corepack |
| Данные во фронте | TanStack Query, TanStack Virtual 3.14 | | виртуализация списков обязательна |
| UI-компоненты | shadcn/ui + Tailwind CSS | | без тяжёлых CSS-эффектов (WebKitGTK) |
| Роутинг | React Router (library mode) | 7.x | |
| i18n | react-i18next | | языки `en`, `ru` |
| ISO9660 | свой код (ADR-0016) | — | ридер повторяет isofs OPL; генератор фикстур в `yaoplm-test-support` |
| CHD | chd | 0.4 | pure Rust, за фичей `chd` |
| Архивы | zip 9, sevenz-rust2 0.24 | | RAR — `unrar` за фичей `rar` (лицензия!) |
| Хеши | crc32fast 1.5, sha1 0.11, md-5 0.11 | | один проход чтения, параллельные хешеры |
| ZSO | lz4_flex 0.14 | | формат пишем сами (24-байтный заголовок); режима LZ4 HC нет, поэтому файлы чуть больше, чем у `ziso.py -c 9` |
| Картинки | image 0.25, fast_image_resize 6 | | PNG, позже 8-битная палитра для RiptOPL |
| XML / YAML | quick-xml 0.42; serde_norway или serde_yaml_ng | | `serde_yaml` архивирован, `serde_yml` небезопасен — не используем |
| HTTP | reqwest 0.13 (rustls) | | ретраи, лимиты запросов, ETag |
| SQLite | rusqlite (bundled) | | только кэш, источник истины — файлы на устройстве |
| Диски | windows-sys 0.61, rustix 1.1 | | тома, ФС, кластер, серийный номер, шина, предвыделение; напрямую через API ОС (ADR-0019) |
| SMB | smb 0.12 | | молодой крейт, за фичей `smb`, later |
| Тесты UI | Playwright, vitest | | e2e против `yaoplm-server` |

Фичи Cargo: `chd`, `rar`, `smb`, `hdd`. Сборка по умолчанию остаётся лёгкой и без проблемных лицензий.

## 3. Структура репозитория

```
yaoplm/
├─ Cargo.toml                 # [workspace]: crates/*, apps/cli, apps/server, apps/desktop/src-tauri
├─ rust-toolchain.toml
├─ package.json  pnpm-workspace.yaml
├─ crates/
│  ├─ yaoplm-core/            # Game, Library, Device, Profile; сканер; JobManager-примитивы; SQLite-кэш; ProgressSink
│  ├─ yaoplm-formats/         # iso9660 + system_cnf, ul_cfg + usba_crc32, cfg, zso, cue/bin, chd, archives, art naming, vmc, cht, apps
│  ├─ yaoplm-hash/            # многохешевый конвейер, Logiqx/Redump DAT, сопоставление
│  ├─ yaoplm-device/          # тома, тип ФС, копирование/нарезка/предвыделение, фрагментация (по ОС)
│  ├─ yaoplm-net/             # ArtSource/MetadataSource, кэш, лимиты, ресайз
│  ├─ yaoplm-hdd/             # later: APA/PFS
│  ├─ yaoplm-api/             # Request/Response/JobEvent (serde + ts-rs), dispatch(), JobManager
│  └─ yaoplm-test-support/    # фикстуры: крошечный ISO с SYSTEM.CNF, BIN/CUE, ZSO, DAT, образ FAT32, «виртуальный» большой файл
├─ apps/
│  ├─ cli/                    # бинарник `yaoplm`
│  ├─ server/                 # бинарник `yaoplm-server` (/api/rpc, /api/events, /media/*, SPA через rust-embed, /health)
│  └─ desktop/src-tauri/      # Tauri 2: команды rpc + subscribe, протокол для обложек, плагины dialog/updater/single-instance
├─ web/                       # SPA для desktop и server
│  ├─ src/api/                # transport.ts, generated/ (выход ts-rs, не править руками)
│  ├─ src/features/           # library, game, devices, jobs, settings
│  ├─ src/locales/{en,ru}/
│  └─ tests/                  # vitest; e2e/ (Playwright против server)
├─ data/                      # встроенные базы (генерируются скриптом, с ATTRIBUTION.md)
├─ packaging/                 # docker/, flatpak/, sidecars/ (later)
├─ fuzz/                      # cargo-fuzz: парсеры ISO/CFG/ul.cfg/ZSO/CUE
├─ docs/
└─ .github/workflows/         # ci.yml, release-desktop.yml, release-cli-server.yml
```

## 4. Слой API

- Каждая операция — вариант `Request` в `yaoplm-api`. Например: `OpenDevice`, `ScanLibrary`, `PlanImport`, `ExecutePlan`, `RenameGame`, `FetchArt`, `ReadCfg`, `WriteCfg`, `CancelJob`.
- `dispatch(Request) -> Result<Response, ApiError>`. Долгие операции сразу возвращают `JobId`, а прогресс приходит потоком `JobEvent`.
- Транспорты:
  - **desktop**: команда Tauri `rpc(req)` и `subscribe(channel)` через `tauri::ipc::Channel`;
  - **server**: `POST /api/rpc` и WebSocket `/api/events`;
  - **CLI**: `yaoplm rpc '<json>'` плюс удобные подкоманды поверх того же `dispatch`.
- Фронтенд выбирает транспорт через `isTauri()`. Каждая функция пишется один раз.
- TS-типы генерирует `ts-rs` в `web/src/api/generated/`. CI проверяет, что сгенерированное совпадает с закоммиченным.

## 5. Модель заданий и безопасность записи

1. **Plan.** Ядро строит `Plan` — список шагов (копировать, нарезать, переименовать, записать CFG, скачать обложку, удалить `games.bin`…), с объёмом данных и предупреждениями. На диск ничего не пишется.
2. **Confirm.** Интерфейс показывает план. Удаления и перезаписи подсвечиваются.
3. **Execute.** `JobManager` выполняет план с токеном отмены и событиями прогресса.
   - Большие файлы пишутся в `*.yaoplm-part`, затем переименовываются.
   - Маленькие файлы (CFG, `ul.cfg`): временный файл, `fsync`, переименование, плюс резервная копия.
   - Каждая транзакция плана (одна игра, одно переименование) ведёт журнал в `.yaoplm/journal/` на устройстве: до точки фиксации сбой откатывается, после неё докатывается (ADR-0021). Резервные копии CFG — в `.yaoplm/backup/`.
   - Задания записи выполняются по одному, в порядке очереди: параллельная запись на один носитель фрагментирует игры.
4. Все пути проходят нормализацию и проверку, что они внутри корня устройства. Никаких `..` и симлинков наружу.
5. Удаление пользовательских файлов происходит только по явному подтверждению. В CLI для этого нужен флаг `--yes`.

## 6. Хранение состояния

- Каталог данных приложения (`dirs`): `settings.toml`, `cache.sqlite`, `art-cache/`, `db/` (скачанные DAT).
- В SQLite лежат:
  - индекс библиотек;
  - кэш хешей с ключом путь+размер+mtime;
  - манифест источников: ETag, дата, лицензия, атрибуция;
  - результаты «не найдено» с TTL.
- Источник истины — файлы на устройстве. Кэш можно удалить без потерь.

## 7. Источники данных

- Трейты `ArtSource`, `MetadataSource`, `DatSource`. У каждого источника: id, лицензия и атрибуция, лимит запросов, кэш, режим офлайн-зеркала (локальный git-клон или zip).
- Порядок для MVP:
  1. ID из образа;
  2. встроенная база названий;
  3. обложки xlenore (raw-URL) → libretro-thumbnails;
  4. Redump DAT (v0.2).
- Запрещено: приватный сервер OPL Manager, скрейпинг сайтов без API, бандл изображений.
- Загрузчики (LDR-1, LDR-2) — тоже источник данных: каталог сборок с репозиторием, лицензией и шаблоном имени ассета; загрузка только с официальных релизов по действию пользователя, проверка хеша, кэш (ADR-0014).

## 8. Устройство и запись (yaoplm-device)

- Обнаружение томов через API ОС (ADR-0019): ФС, свободное место, кластер, сектор, серийный номер, шина, раздел.
- Копирование: последовательная запись большими блоками, предвыделение (по ОС: `fallocate`/`F_PREALLOCATE`/`SetFileInformationByHandle`), расчёт скорости.
- Нарезка `ul`: ровно 1 ГиБ (0x80000 секторов по 2048 байт) на чанк, кроме последнего.
- Фрагментация (v0.2): Windows `FSCTL_GET_RETRIEVAL_POINTERS`, Linux `FIEMAP`, macOS `F_LOG2PHYS_EXT`.
- Форматирование съёмных носителей (DEV-3, v0.2) и сырой доступ к дискам (HDD, later) — только через отдельный процесс `yaoplm helper --stdio` с повышенными правами (ADR-0013). Основной процесс строит план, помощник выполняет его после повторной проверки диска.
- Профили носителей: USB, MX4SIO, SD эмулятора карты памяти (MMCE), iLink, HDD в exFAT. Профиль задаёт ФС, разметку, кластер и куда ставить загрузчик (SPEC §4, LDR-3).

## 9. Режим сервера

- По умолчанию слушает `127.0.0.1`. Для LAN нужно явно указать `--bind 0.0.0.0` и задать токен.
- Доступ только к корням из allow-list (`--root /mnt/ps2`).
- Docker: многоэтапная сборка (web → musl-бинарник → distroless/static), образы для amd64 и arm64 в GHCR.

## 10. Тестирование, CI, упаковка

Тестирование описано в [TESTING.md](TESTING.md). Кратко про CI:
- `ci.yml`, на каждый PR:
  - Rust: `fmt` и `clippy -D warnings`;
  - тесты на Windows, Linux и macOS;
  - web: lint, typecheck, vitest;
  - Playwright против `yaoplm-server`;
  - проверки зависимостей ядра;
  - проверка свежести сгенерированных TS-типов.
- `release-desktop.yml` собирает `tauri-action@v1` под Windows (NSIS/MSI), macOS universal (dmg, ad-hoc подпись) и Linux (AppImage, deb, rpm).
- `release-cli-server.yml` собирает бинарники и Docker-образ.
- Подпись: SignPath Foundation для Windows (после первого релиза), Apple Developer ($99 в год) — когда появятся пользователи. Ключ updater хранится в GitHub Secrets и **нигде больше**.
