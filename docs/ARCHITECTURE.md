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
│  ├─ yaoplm-formats/         # iso9660 + system_cnf, ul_cfg + usba_crc32, cfg, zso, cue/bin, chd, archives, art naming, vmc, ps2mc + ps1mc + icon_sys (memory cards), sd2psx_ini, cht, apps
│  ├─ yaoplm-hash/            # многохешевый конвейер, Logiqx/Redump DAT, сопоставление
│  ├─ yaoplm-i18n/            # Phrase: ключ каталога + параметры, английский текст из web/src/locales/en (ADR-0029)
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

- Каждая операция — вариант `Request` в `yaoplm-api`. Например: `ListDevices`, `InspectDevice`, `ScanLibrary`, `ListDir`, `PlanImport`, `ExecutePlan`, `RenameGame`, `FetchArt`, `FindArt`, `ImportArt`, `SetArt`, `DeleteArt`, `Attribution`, `ReadCfg`, `WriteCfg`, `CancelJob`.
- `FetchArt` — задание, которое заканчивается планом (`JobResult::Plan`): поиск обложек идёт минутами и ведёт журнал по играм (`JobLogEntry::Art`), а запись на носитель остаётся планом с подтверждением (ADR-0025). Типы картинок задаёт `kinds` (ADR-0032). С `import: <id плана импорта>` задание заканчивается набором обложек (`JobResult::Covers`), который `PlanImport.covers` встраивает в план добавления (ADR-0031). `FindArt` — задание, которое возвращает варианты одной картинки на выбор (`JobResult::ArtVariants`); `ImportArt` читает наборы картинок пользователя и заканчивается планом (ADR-0033). Сеть используется только внутри заданий `FetchArt` и `UpdateRedump`: `yaoplm-api` держит tokio-runtime и вызывает `yaoplm-net` из потока задания.
- `VerifyGames` — задание только на чтение: хеши образов, вердикты по DAT Redump, строка журнала на игру (`JobLogEntry::Verify`), результат `VerificationReport`. `RedumpStatus` и `UpdateRedump` (загрузка не чаще раза в неделю или импорт файла) управляют DAT (ADR-0026). `ConvertGames` возвращает план конвертации ISO ↔ ZSO ↔ ul (ADR-0027); `PlanImport` с `compress` пишет образы сразу в ZSO, а источники в zip и 7z читаются через `ArchiveSources` (ADR-0028). `PlanImport` с `verify` хеширует образы в том же проходе, что и запись; `ExecutePlan` кладёт хеши в кэш и отдаёт вердикты в `ExecutionReport.checks` (ADR-0030).
- Тексты для пользователя ядро отдаёт фразами (ADR-0029): `Phrase { key, params }` рядом с английским текстом — у заголовков планов, транзакций и заданий (`titlePhrase`, `itemPhrase`), у уведомлений, предупреждений, советов и причин пропуска (`phrase`), у строк журнала (`detailPhrase`, `errorPhrase`) и у ошибок (`ApiError.phrase`). Английский текст рендерится из каталога `en` интерфейса (`yaoplm-i18n`), перевод делает клиент.
- Загрузчики (LDR-1, LDR-2, ADR-0042, ADR-0043): `LoaderCatalog` отдаёт действующий каталог (из `main`, если он прочитан этой версией программы, иначе вшитый), `LoaderFiles` — сохранённые ELF. Задания `LoaderRelease` (что будет скачано, `JobResult::LoaderOffer`), `DownloadLoader` (держится SHA-256, который видел пользователь, `JobResult::LoaderFile`), `ImportLoader` (свой файл без сети) и `UpdateLoaderCatalog` (`JobResult::LoaderCatalog`); сеть — через `yaoplm_net::loaders`, у каждого запроса может быть `mirror`. Установка (LDR-3, BOOT-1, ADR-0044): `PlanInstallLoader` — план из сохранённого ELF (папки → ELF на каждом пути с резервной копией прежнего → языковой пакет → манифест `.yaoplm/manifest.json` → стартовые настройки), `LoaderBootHints` — строки для лаунчеров, `LoaderLanguages` — языки пакета; ядро — `yaoplm_core::loaders::{install, boot, languages, manifest}`. Обновление и откат (LDR-4, ADR-0045): `DeviceInfo.installed` — установленные сборки с последней известной версией канала из сохранённых ответов GitHub (без сети), `CachedLoaderRelease` — сведения о релизе из того же кэша, `PlanUpdateLoader` — план установки с `update` (только ELF, языковой пакет и манифест), `PlanRollbackLoader` — обмен ELF и последней резервной копии; ядро — `loaders::{installed, rollback}`. Выбор и замена сборки (LDR-6, LDR-7, ADR-0046): задание `LoaderReleases` — релизы канала, которые разрешает каталог; задание `LoaderImpact` — библиотека по правилам новой сборки (`loaders::impact`); `PlanReplaceLoader` — один план из готовых (конвертации, картинки, установка), собранный `Plan::absorb`.
- Настройки игры и карты памяти (CFG-2, VMC-1, ADR-0036): `ReadCfg` отдаёт настройки и все ключи CFG игры, `WriteCfg` строит план правки (настройки по правилам меню OPL, ключи как есть, новые карты в той же транзакции), `ListVmc` перечисляет `VMC/` с вердиктом OPL, числом сейвов и играми, `CreateVmc` планирует пустые карты. Транзакция плана несёт список ключей «было → стало» (`cfgChanges`), итог выполнения — уведомления после записи (`ExecutionReport.notices`, например карта в нескольких кусках).
- `dispatch(Request) -> Result<Response, ApiError>`. Долгие операции сразу возвращают `JobId`, а прогресс приходит потоком `JobEvent`. Планы ещё и ведут журнал шагов (`JobEvent::Log`: транзакция или шаг начат, выполнен, откачен, провален); `GetJob` отдаёт его целиком, поэтому переподключившийся клиент ничего не теряет.
- `ListDir` показывает папки для выбора источников в браузере: сервер отдаёт только свои корни и то, что внутри них (`PathPolicy`), десктоп — тома; скрытые файлы и симлинки не показываются.
- Транспорты:
  - **desktop**: команда Tauri `rpc(req)` и `subscribe(channel)` через `tauri::ipc::Channel`;
  - **server**: `POST /api/rpc` и WebSocket `/api/events`;
  - **CLI**: `yaoplm rpc '<json>'` плюс удобные подкоманды поверх того же `dispatch`.
- Фронтенд выбирает транспорт через `isTauri()`. Каждая функция пишется один раз.
- Связь интерфейса с ядром может пропасть только в браузере: в десктопе ядро в том же процессе. Поэтому состояние связи показывается только для HTTP-транспорта, и только когда связи нет (план UI-4, `docs/plans/m5-ui-simple-mode.md`, задача 8). Раздача папки для PS2 по SMB — дело ОС или NAS, а не этой связи.
- TS-типы генерирует `ts-rs` в `web/src/api/generated/`. CI проверяет, что сгенерированное совпадает с закоммиченным.

## 5. Модель заданий и безопасность записи

1. **Plan.** Ядро строит `Plan` — список шагов (копировать, нарезать, переименовать, записать CFG, отформатировать карту памяти, скачать обложку, удалить `games.bin`…), с объёмом данных и предупреждениями. На диск ничего не пишется.
2. **Confirm.** Интерфейс показывает план. Удаления и перезаписи подсвечиваются.
3. **Execute.** `JobManager` выполняет план с токеном отмены и событиями прогресса.
   - Большие файлы пишутся в `*.yaoplm-part`, затем переименовываются.
   - Маленькие файлы (CFG, `ul.cfg`): временный файл, `fsync`, переименование, плюс резервная копия.
   - Каждая транзакция плана (одна игра, одно переименование) ведёт журнал в `.yaoplm/journal/` на устройстве: до точки фиксации сбой откатывается, после неё докатывается (ADR-0021). Резервные копии CFG — в `.yaoplm/backup/`.
   - Задания записи выполняются по одному, в порядке очереди: параллельная запись на один носитель фрагментирует игры.
4. Все пути проходят нормализацию и проверку, что они внутри корня устройства. Никаких `..` и симлинков наружу.
5. Удаление пользовательских файлов происходит только по явному подтверждению. В CLI для этого нужен флаг `--yes`.

## 6. Хранение состояния

- Каталог данных приложения (`dirs`, переопределяется `YAOPLM_DATA_DIR` и `--data-dir` сервера): `cache.sqlite`, `art-cache/<source>/<key>/` (оригинал и производные PNG; `art-cache/local/` — картинки из файлов и наборов пользователя; `art-cache/oplm-index/<item>/` — центральный каталог копии OPL Manager, ADR-0035), `db/redump-ps2.dat` с метаданными `db/redump-ps2.json` и отметкой последнего запроса `db/redump-ps2.asked` (с M5, ADR-0026), `loaders/` (ELF загрузчиков `elf/<sha256>.elf` с описанием, языковые пакеты `langs/<sha256>`, ответы GitHub о релизах и списки релизов с `ETag`, каталог из `main`; с M5, ADR-0043, ADR-0044, ADR-0046), позже `settings.toml`. Создаётся при первой записи в кэш: сканирование только читает его. Десктоп и CLI: `%LOCALAPPDATA%\yaoplm`, `~/.local/share/yaoplm`, `~/Library/Application Support/yaoplm`.
- В SQLite лежат:
  - манифест обложек: источник, URL, ETag, `Last-Modified`, дата, SHA-256, лицензия, атрибуция (с M4);
  - результаты «не найдено» с TTL 7 дней (с M4);
  - кэш хешей образов (CRC-32, SHA-1, MD5) с ключом «том + путь + размер + mtime» каждого файла (с M5, ADR-0026); очистка кэша обложек его не трогает;
  - хеши сырой дорожки данных CD по паре «SHA-1 и размер образа» (таблица `tracks`, с M5, ADR-0030);
  - позже: индекс библиотек.
- Встроенная база названий (META-1) не лежит в каталоге данных, а вшита в бинарник: `data/titles.json.gz` (ADR-0024).
- Источник истины — файлы на устройстве. Кэш можно удалить без потерь.
- Настройки интерфейса (профили, выбранные для носителей, стиль имён, открытый корень, вид библиотеки, проверка образов) с M3 хранятся в `localStorage` браузера или WebView (`yaoplm.settings`). `settings.toml` появится, когда настройки понадобятся CLI и серверу.

## 7. Источники данных

- `yaoplm-net`: один HTTP-клиент (User-Agent `YAOPLM/<версия> (+repo)`, не чаще одного запроса в секунду на источник, условные и Range-запросы, back-off на 429/5xx, `YAOPLM_OFFLINE=1` запрещает всё, кроме этого компьютера), кэш с манифестом и источники картинок `SourceId::{Xlenore, Oplm, Libretro, ScreenScraper}` (ADR-0035); у каждого: лицензионная метка, атрибуция и режим зеркала (папка с раскладкой репозитория, она же мок для тестов), кроме ScreenScraper. Порядок источников свой у каждого типа (`sources::order`), политика регионов — `region`. Метаданные берутся из встроенной базы, `DatSource` появился с META-2.
- Порядок для MVP:
  1. ID из образа;
  2. встроенная база названий;
  3. картинки по типам (ADR-0035): xlenore (raw-URL по серийнику), копия OPL Manager на archive.org (оглавление zip один раз, затем Range-запрос на картинку), libretro-thumbnails (`thumbnails.libretro.com`, затем raw GitHub, по имени Redump из встроенной базы), ScreenScraper при ключе разработчика; никакого GitHub API;
  4. Redump DAT (v0.2).
- Запрещено: приватный сервер OPL Manager, скрейпинг сайтов без API, бандл изображений.
- Загрузчики (LDR-1, LDR-2) — тоже источник данных: каталог сборок `data/loaders/` (индекс и файл на сборку: семейство, канал, репозиторий, лицензия, профиль, ассеты, источники хешей для проверки загрузки, папки, пресеты путей, отпечатки с шаблонами версий, описания на каждом языке). Каталог читает ядро (`yaoplm_core::loaders`: вшитая копия, строгий разбор со схемой). Истории хешей новых сборок нет: сборку на носителе узнаёт версия внутри ELF (ADR-0041), а таблица `data/loader-hashes/opl-stable.txt` — только для замороженных выпусков. Загрузка только с официальных релизов по действию пользователя, проверка хеша, кэш (ADR-0014, ADR-0022, ADR-0042).

## 8. Устройство и запись (yaoplm-device)

- Обнаружение томов через API ОС (ADR-0019): ФС, свободное место, кластер, сектор, серийный номер, шина, раздел.
- Копирование: последовательная запись большими блоками, предвыделение (по ОС: `fallocate`/`F_PREALLOCATE`/`SetFileInformationByHandle`), расчёт скорости.
- Нарезка `ul`: ровно 1 ГиБ (0x80000 секторов по 2048 байт) на чанк, кроме последнего.
- Фрагментация (v0.2): Windows `FSCTL_GET_RETRIEVAL_POINTERS` (`DeviceFs::fragments`, с M5 проверяет новые VMC после записи), Linux `FIEMAP`, macOS `F_LOG2PHYS_EXT` (вместе с проверкой библиотеки, IMP-7b).
- Форматирование съёмных носителей (DEV-3, v0.2) и сырой доступ к дискам (HDD, later) — только через отдельный процесс `yaoplm helper --stdio` с повышенными правами (ADR-0013). Основной процесс строит план, помощник выполняет его после повторной проверки диска.
- Профили носителей: USB, MX4SIO, SD эмулятора карты памяти (MMCE), iLink, HDD в exFAT. Профиль задаёт ФС, разметку, кластер и куда ставить загрузчик (SPEC §4, LDR-3).

## 9. Режим сервера

- По умолчанию слушает `127.0.0.1`. Для LAN нужно явно указать `--bind 0.0.0.0` и задать токен.
- Доступ только к корням из allow-list (`--root /mnt/ps2`); зеркала источников обложек тоже должны лежать внутри корней.
- `GET /media/art?path=` отдаёт обложки для `<img>`: только `ART/<ID>_<SUFFIX>.png|jpg|bmp` внутри корней, с `ETag`; токен принимается в `?token=`, как у `/api/events` (ADR-0012).
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
- `release-desktop.yml` собирает `tauri-action@v1` под Windows (NSIS/MSI), macOS universal (dmg, ad-hoc подпись) и Linux (AppImage, deb, rpm) только по тегу `v*` (минуты приватного репозитория, ADR-0011) и публикует черновик пре-релиза. Updater не подключён: ключа подписи нет (CLAUDE.md, правило 5).
- `release-cli-server.yml` собирает бинарники и Docker-образ.
- Подпись: SignPath Foundation для Windows (после первого релиза), Apple Developer ($99 в год) — когда появятся пользователи. Ключ updater хранится в GitHub Secrets и **нигде больше**.
