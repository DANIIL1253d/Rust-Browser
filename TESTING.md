# Проверка Rust Browser

Экспорт чистого комплекта проверяется без запуска браузера и чтения профилей:

```powershell
python -m unittest discover -s tests/distribution -v
```

Проверяются состав файлов, SHA-256, отказ при небезопасных путях, приватных файлах,
отсутствующих исходниках и уже существующей папке назначения.

Проверки выполняются в Windows x64 с MSVC. Для сборки приложения нужны Rust
stable, Windows SDK, Python 3.11+ и SDK из `Setup-MediaTools.ps1`. Не запускай
несколько Cargo в одной папке target одновременно.

`Build-Browser.ps1` учитывает `CARGO_TARGET_DIR`. На Windows длинный путь к target
может превысить ограничение команды линковки ANGLE. Используй короткий путь или
временный `subst` для папки target; удаляй такое сопоставление после сборки.

```powershell
.\Build-Browser.ps1 -CoreTests -PythonPath C:\Path\To\python.exe
.\Build-Browser.ps1 -UiTests -PythonPath C:\Path\To\python.exe
.\Build-Browser.ps1 -NetworkTests -PythonPath C:\Path\To\python.exe
```

Тесты проверяют профили, сохранение изменений, подписки, DPAPI, CSV, состояние
медиа, планирование перерисовки и сведения о соединении. Сетевые проверки
используют локальные серверы. Отдельный необязательный запрос к YouTube
не проверяет воспроизведение видео.

Все команды `Build-Browser.ps1` используют `--locked`. Версия `+stable` пока
не совпадает с закреплённой в `rust-toolchain.toml`; это не воспроизводимый
toolchain gate. Не интерпретируй быстрые routing-тесты как весь сетевой набор:

```powershell
cargo +stable test --locked --offline -p servo-net --test main -- --test-threads=1
```

Наличие ошибок в полном наборе не разрешает выключать тесты или объявлять gate
зелёным по результату только отфильтрованных проверок.

## Собранный движок

Сначала собери и упакуй приложение. Все команды ниже используют отдельные
синтетические профили и локальные страницы. Node.js нужен для тестов JavaScript.
Результаты сохраняются в выбранной рабочей папке и не включаются в Git.

```powershell
python tests/browser-session/lazy_smoke.py --app app/RustBrowser.exe --work .build/session-check
python tests/browser-security/smoke.py --app app/RustBrowser.exe --live
node tests/browser-autofill/engine.mjs app/RustBrowser.exe .build/autofill-check
python tests/browser-storage/smoke.py --app app/RustBrowser.exe --work .build/storage-check
```

Проверка соединения различает HTTP, отвергнутый сертификат, явное отключение
проверки сертификата и ошибку загрузки. `--live` дополнительно проверяет
действительное HTTPS-соединение. Для локального TLS используются тестовые
сертификаты из исходников Servo; их нет в дистрибутиве приложения.

Проверка хранения перезапускает движок и проверяет синтетические HttpOnly-cookie
и localStorage. Она не читает профиль Brave. Тест автозаполнения использует
вымышленные значения и проверяет почту, пароль, замену поля и потерю фокуса.

## Секреты в диагностических логах

```powershell
python tests/browser-security/test_log_privacy.py
cargo +stable test --locked --offline -p servo-net --test main browser_log_privacy_ -- --test-threads=1
python tests/browser-security/log_privacy.py --app C:\Builds\candidate\app\RustBrowser.exe --work .build/privacy-check
```

Последняя команда должна запускать **свежий отдельный упакованный candidate**,
не старый установленный `app`. Она проверяет настоящие INFO/DEBUG/TRACE-логи и реальные
синтетические HTTP-only cookies, Authorization, нестандартные заголовки,
path/query, загрузку картинки и WebDriver navigation. PATH/plugin discovery
изолированы; только собственный профиль и собственное дерево процессов.
Отчёт не копирует значения маркеров. Отключённое логирование, неполный сценарий
или найденный секрет — ошибка, а не PASS. Это не общий аудит всех логов движка;
error paths и отдельные tracing spans требуют дополнительных регрессий.

Rust-набор также проверяет разрешённый и отклонённый CORS-preflight с
нестандартными методами: значения методов и заголовков не попадают в DEBUG,
разрешённый запрос доходит до сервера, отклонённый блокируется как CorsMethod.
Проверка пакета выполняет такие запросы из JavaScript на втором localhost origin
и требует реальной CORS-диагностики. HTTP-only логи без CORS не дают PASS.
WebSocket-сценарии проверяют передачу текста и бинарных данных в обе стороны,
закрытие по инициативе сервера и клиента с сохранением code/reason, отправку
cookies и обработку Set-Cookie из handshake. Для бинарных маркеров учитывается
также десятичное представление байтов в Debug. Отсутствие WS-диагностики — FAIL.
Для TRACE-контракта нужны реальные TRACE-записи `net::websocket_loader` и
`net::fetch::methods`: DEBUG-only лог и слово TRACE внутри текста не дают PASS.

## Аудио и видео

Дополнительные регрессии используют только собственные локальные ролики и отдельные профили:

```powershell
python tests/browser-media/generate.py --sdk C:\Path\To\GStreamer --output .build/media-fixtures
node tests/browser-media/layout.mjs app/RustBrowser.exe .build/layout-check .build/media-fixtures/video.webm 60
node tests/browser-media/repaint.mjs app/RustBrowser.exe .build/repaint-check .build/media-fixtures
node tests/browser-media/lifetime.mjs app/RustBrowser.exe .build/lifetime-check .build/media-fixtures/video.webm
```

Первая проверка требует, чтобы неизменяемая страница не пересчитывала layout на
каждом кадре, и собирает статистику встроенного профайлера. Вторая проверяет
реальные пиксели скриншотов после первого кадра, смены источника при том же
разрешении, изменения размеров, паузы и повторного добавления видео в DOM.
Третья включает `dom_testutils_enabled` только в своём профиле, вызывает GC20раз и
проверяет живое отсоединённое видео и10циклов удаления iframe с воспроизведением.
Это headless-регрессии; они не измеряют FPS окна браузера или конкретного сайта.

```powershell
python tests/browser-media/generate.py --sdk .build/gstreamer-1.28.8 --output .build/media-fixtures
python tests/browser-media/smoke.py --app app/RustBrowser.exe --work .build/media-webm --video .build/media-fixtures/video.webm
python tests/browser-media/smoke.py --app app/RustBrowser.exe --work .build/media-mp4 --video .build/media-fixtures/video.mp4
node tests/browser-mse/engine.mjs app/RustBrowser.exe .build/mse-default
```

Генератор создаёт собственные тестовые данные. Проверка требует продвижения
времени, декодированных пикселей, перемотки и удержания паузы. PATH и пути
плагинов изолированы: установленный SDK не может подменить отсутствующую DLL
в упакованном приложении. Это проверка реального движка в headless-режиме;
она не подтверждает вывод звука на устройство или работу нативного окна.

MediaSource выключен по умолчанию. Для разработки предусмотрен отдельный
[контракт](tests/browser-mse/README.md). Включённый полный контракт пока содержит
ожидаемые ошибки из-за отсутствующих нативных операций; их нельзя считать
успехом или заменять проверками фиктивных диапазонов.

## Длительные измерения

Дождись завершения компиляции и других тяжёлых задач. Сравнивай последовательные
запуски на одной машине с одинаковыми настройками питания:

```powershell
python tests/browser-performance/compare.py --app app/RustBrowser.exe --baseline C:\Builds\previous\RustBrowser.exe --reference C:\Path\To\brave.exe --work .build/comparison --seconds 300
python tests/browser-session/lazy_smoke.py --app app/RustBrowser.exe --work .build/session-benchmark --benchmark
```

Первый сценарий выполняет одинаковую локальную нагрузку и затем ждёт в простое.
Он сохраняет отдельные метрики CPU, рабочего набора и приватной памяти всего
дерева процессов. Сумма рабочих наборов может учитывать общие страницы несколько
раз. Это синтетическое сравнение headless-режимов; оно не доказывает превосходство
на реальных сайтах, плавность нативного интерфейса или предел памяти 500 МБ.

## Упаковка и установщик

```powershell
.\Build-Browser.ps1 -Release -PythonPath C:\Path\To\python.exe
.\Release-Browser.ps1 -Version v0.1.1
.\Setup-InstallerTools.ps1
.\Build-Installer.ps1 -Version v0.1.1
```

Упаковка сверяет SHA-256 EXE с метаданными сборки и включает зависимости
GStreamer, лицензии и разрешённые библиотеки Visual C++ Runtime. Проверки
установщика находятся в `tests/installer`: проверка входных файлов и отдельная
изолированная установка с последующим удалением. Нативный интерфейс, установку
на чистой Windows, внешние сайты и потоковое воспроизведение проверяют отдельно.

## Совместимость с Turnstile

`python tests/browser-compatibility/turnstile.py --app <RustBrowser.exe> --work <scratch>` запускает официальный dummy-виджет Cloudflare в отдельном профиле. Проверяет JavaScript, cookies, localStorage и одинаковый User-Agent в HTTP и JavaScript. Успех тестового виджета не подтверждает прохождение реальных проверок сайтов. Методика: https://developers.cloudflare.com/turnstile/troubleshooting/testing/.
