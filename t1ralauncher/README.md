# T1raLauncher (Rust + Tauri v2)

Лаунчер Minecraft с раздельными сборками (как в Prism/XMCL): у каждой сборки своя папка с модами и мирами, а библиотеки, ресурсы и клиентские jar'ы общие.

## Что умеет

- Сборки: Vanilla, Fabric, Quilt (любая версия Minecraft из манифеста Mojang, включая снапшоты)
- Параллельная загрузка (16 потоков) с проверкой SHA1, докачка только недостающего
- Старые версии: нативы старого формата, виртуальные ассеты
- Оффлайн-аккаунт (ник + стандартный offline UUID)
- Память на сборку, свои JVM-аргументы, свой путь к Java
- Зеркало BMCLAPI как запасной источник, если Mojang недоступен
- Консоль игры в реальном времени, остановка процесса, открытие папки сборки

Не реализовано: Forge/NeoForge (нужен запуск их установщика), вход через Microsoft, автоскачивание Java, импорт TLauncher.

## Сборка

1. Установи [Rust](https://rustup.rs) и Tauri CLI: `cargo install tauri-cli --version "^2"`
2. Linux: пакеты `libwebkit2gtk-4.1-dev build-essential libssl-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev`. Windows: WebView2 (есть в Windows 10/11) и Visual Studio Build Tools.
3. Иконки (один раз, свои из любого PNG 1024×1024): `cargo tauri icon src-tauri/icons/icon.png`
4. Запуск в режиме разработки: `cd src-tauri && cargo tauri dev`
5. Релиз: `cd src-tauri && cargo tauri build` (готовый установщик в `src-tauri/target/release/bundle/`)

Интерфейс сделан на чистом HTML/JS (папка `ui/`), поэтому Node.js и npm не нужны.

## Где лежат данные

- Windows: `%APPDATA%\T1raLauncher`
- Linux: `~/.local/share/T1raLauncher`
- macOS: `~/Library/Application Support/T1raLauncher`
- Портативный режим: задай переменную `T1RA_HOME=папка`

```
T1raLauncher/
├── launcher.json          настройки
├── instances/<id>/        instance.json + .minecraft/ (mods, saves, config...)
├── libraries/             общие библиотеки
├── assets/                общие ресурсы (indexes + objects)
├── versions/<id>/         client.jar и манифесты
└── meta/                  кэш списка версий
```

## Java

Нужна установленная Java: 8 для старых версий, 17 для 1.18–1.20.4, 21 для 1.20.5 и новее. Если в PATH не та версия, укажи путь к нужной в настройках.
