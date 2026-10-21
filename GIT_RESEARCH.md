Я перевірив репозиторії через дерево файлів GitHub та сирий код, тому шляхи нижче справжні. Усе, що я не зміг відкрити, позначено окремо.

Три речі, які варто знати до читання
Wayland. За обговоренням у Tauri, always-on-top на Wayland не працює, і мейнтейнери закрили issue як таке, що не лежить у їхніх руках. У Mozilla та сама причина: на Wayland вікна не можуть самі ставати «поверх усіх». OpenPet на Linux автор вручну не перевіряв (його README). Для MVP на Linux реалістичний шлях (моя оцінка, не перевірена в репозиторіях) — XWayland або X11. 
github
mozilla
OAuth для YouTube Music. За документацією Music Assistant, з листопада 2024 Google прибрав OAuth для YT Music, тож лишається лише автентифікація cookie-файлами. Це збігається з твоєю вимогою не лізти в OAuth. 
music-assistant
victauri-browser — не брати. Я знайшов його в пошуку, але у README репозиторію (4DA-Systems/Victauri) сказано, що браузерний режим видалено 2026-06-09. Лишився тільки плагін для Tauri-додатків.
1. Desktop-маскоти
X-T-E-R/OpenPet — https://github.com/X-T-E-R/OpenPet
Стек: Tauri v2, Rust (HTTP-сервер на голому std::net), React/Vite/TS. Ліцензія GPL-3.0-or-later.
Ключова фіча:
Click-through без нативних хаків: кожні 80 мс фронтенд читає cursorPosition(), позицію вікна й scale factor, перевіряє попадання курсора в bounding box спрайта й перемикає setIgnoreCursorEvents(...).
Drag: поріг 4 px, далі нативний startDragging(), а якщо він упав — ручний setPosition. Після drag клік придушується.
Автоблукання: тік 120 мс, робоча область монітора оновлюється кожні 4 с, на hover і drag пауза.
Агентні події (thinking, tool-running, success…) приходять через локальний HTTP.
Що вивчити:
src/PetWindow.tsx (hit-test, drag, рух)
src/pet/motion.ts (resolvePetMotion, clampPetMotionToWorkArea)
src/pet/animation.ts
src-tauri/tauri.conf.json (transparent, decorations:false, alwaysOnTop, skipTaskbar, shadow:false, macOSPrivateApi)
src-tauri/src/http_api.rs
skills/openpet-mcp/scripts/openpet_mcp_server.py
Плюси / пастки:
Плюс: найкоротший робочий приклад саме твого вікна.
Мінус: polling 12 разів на секунду з трьома IPC-викликами, і хіт-тест по прямокутнику, а не по пікселях.
Не копіюй http_api.rs: він без автентифікації, віддає Access-Control-Allow-Origin: *, а accept-цикл послідовний. Будь-яка веб-сторінка може слати POST на localhost.
macOSPrivateApi закриває шлях в App Store.
Qrytics/littleGuy — https://github.com/Qrytics/littleGuy

Невеликий прототип: 2 зірки, 21 коміт.

Стек: Tauri v2, Rust, React/TS.
Ключова фіча: реакція на активне вікно користувача.
Що вивчити:
src-tauri/src/activity_monitor.rs
src-tauri/src/ipc_bridge.rs
src/components/Overlay.tsx
Пастки: README декларує Linux-детекцію через xdotool/xprintidle, а це X11. Твердження «прозорі пікселі click-through» і анімаційний рушій у README я в коді не перевіряв.

Не перевірені лиди (сторінки не відкрились, шляхи лише зі сніпетів пошуку): wizard404error/sakura (Tauri 2, Live2D, src-tauri/src/{tray,window}.rs, ai/prompt.rs) та meet447/Meuxe (Tauri 2 з ACP-клієнтом).

2. Керування браузером без Selenium
Vivswan/chromium-bridge — https://github.com/Vivswan/chromium-bridge

API GitHub веде на Vivswan/genkan: репозиторій, схоже, перейменували.

Стек: один Rust-бінарник у трьох режимах (MCP-сервер/брокер, --native-host, CLI), MV3-розширення на TypeScript (WXT), Tauri-додаток для керування.
Ключова фіча:
Ланцюжок: MCP-клієнт → stdio → брокер → Unix-сокет (перевірка peer-UID, kernel attestation, HMAC) → native host → chrome.runtime.connectNative → розширення. Без CDP-порту.
chrome.debugger використовується тільки для точного дерева доступності.
Захисні шари: підтвердження ризикових дій, allowlist сайтів, kill switch, аудит-лог.
Що вивчити:
src/apps/extension/src/lib/background/port.ts: автореконект із паузою 2 с, бо MV3 service worker вбивається приблизно кожні 5 хв; з'єднання нумеруються, і кадри зі старого порту відкидаються.
…/background/dispatch.ts, confirm/gate.ts
…/background/backends/{content-script,cdp}.ts
src/apps/extension/src/lib/dom/page-api.ts (клік, fill, snapshot)
src/packages/core/src/tools/catalogue.rs, protocol.rs, broker.rs
Плюси / пастки:
Найстрогіша модель безпеки з усього, що я бачив.
Для «клікни Play у вкладці» це надмірно.
Ліцензія нестандартна («Individual and Small Organization License 1.0.0» плюс частини Apache), перед запозиченням коду прочитай її.
Windows тримається на loopback TCP та одному HMAC-секреті.
Користувач вручну завантажує розширення й реєструє manifest native host для кожного браузера.
raychao-oao/firefox-bridge — https://github.com/raychao-oao/firefox-bridge
Стек: Node.js (native host і MCP-сервер) плюс WebExtension. Ліцензія MIT.
Ключова фіча: той самий патерн у компактному читабельному вигляді. Native host запускає браузер, MCP-сервер запускає клієнт, тож їм потрібен IPC. Host піднімає Unix-сокет або named pipe з токеном (32 байти hex у файлі 0600, каталог 0700). Кожне з'єднання проходить auth і отримує сесію. Помилка кадрування вбиває лише це з'єднання. Є tab-lease для кількох сесій, policy gate і передача великих даних (скріншоти) через тимчасові файли.
Що вивчити:
native-host/src/native-messaging.js (4-байтний LE-префікс довжини, ліміт 1 MiB на ланці браузера)
native-host/src/socket-server.js
native-host/src/payload-store.js
extension/policy-gate.js
mcp-server/src/bridge-client.js
Пастки: тільки Firefox. Автотестів розширення немає (README відсилає до ручного чекліста), проєкт ранній.
3. YouTube Music / Spotify і MPRIS
SimoHypers/limusic — https://github.com/SimoHypers/limusic
Стек: Tauri v2, Rust-workspace, SvelteKit, libmpv. Ліцензія GPL-3.0.
Ключова архітектура:
MPRIS ззовні: souvlaki 0.8.3 живе у виділеному потоці, бо MediaControls не Send. Застосунок шле оновлення каналом (MediaHandle), а натискання з ОС іде в ті самі методи AppState, що й кнопки UI.
YouTube Music без OAuth: cookie з логін-вебв'ю, SAPISIDHASH, а ротацію __Secure-*SIDTS обробляє transport. Кожен Set-Cookie зливається в jar, оновлений jar пишеться назад у SQLite, а account_menu раз на півгодини тримає сесію живою.
Що вивчити:
src-tauri/src/media.rs
src-tauri/src/session.rs, docs/SIGN-IN-SESSIONS.md
crates/innertube/src/endpoints.rs: playlist(browse_id), next(video_id, playlist_id), library_grid, rate, home, history
crates/innertube/src/{transport,clients}.rs
src-tauri/src/cipher/ та src-tauri/src/potoken/ (розшифрування стрімів)
Liked Music / Supermix: явних констант LM чи RDMM у endpoints.rs я не знайшов. Але playlist() приймає будь-який browseId, а next() приймає playlistId без videoId. playlist_suggestions використовує радіо-id вигляду RDAMPL…. Імовірно, VLLM і RDMM проходять тими ж методами, але це не підтверджено.
Плюси / пастки:
GPL-3.0: читати як референс можна, а переносити код у не-GPL проєкт не можна.
Це reverse-engineered API: стрім вимагає PO token і запуску BotGuard через deno_core (V8).
souvlaki не дає TrackList, тож керування чергою через MPRIS цим шляхом не зробиш (це моє знання про бібліотеку).

Для твого сценарію («відкрити персональний стрім») є легший шлях, моя пропозиція, не перевірена в коді. Відкривати URL у браузері, де користувач уже залогінений, і керувати плеєром через MPRIS браузера. Тоді реалізовувати клієнт YouTube Music не треба взагалі.

aome510/spotify-player — https://github.com/aome510/spotify-player
Стек: Rust, TUI, rspotify, librespot, souvlaki.
Ключова фіча: src/media_control.rs. Події (Play/Pause/Toggle/Next/Previous/SetPosition/SetVolume/OpenUri) перетворюються на ClientRequest::Player(...) і йдуть у канал flume. Метадані оновлюються лише при зміні треку. На Linux цикл оновлення не частіший за 1 с, бо, за коментарем у коді, обробник D-Bus у souvlaki приймає одну подію на секунду. На Windows потрібне фіктивне вікно й pump_event_queue. Відповідно до README, медіа-керування це MPRIS DBus на Linux і події вікон на Windows/macOS. 
github
Що вивчити: spotify_player/src/media_control.rs, client/mod.rs, auth.rs, token.rs, cli/{commands,handlers}.rs.
Пастки: потрібен Spotify Premium і власний зареєстрований застосунок; Spotify тут справжній API, без хаків.
Spotifyd/spotifyd — https://github.com/Spotifyd/spotifyd
Стек: Rust, dbus-crossroads, dbus-tokio, librespot.
Ключова фіча: повноцінний сервер MPRIS v2 у src/dbus_mpris.rs.
Реєструє org.mpris.MediaPlayer2 та .Player.
Методи: Play/Pause/PlayPause/Next/Previous/Stop/Seek/SetPosition/OpenUri.
SetPosition ігнорує застарілий track_id, як вимагає специфікація.
Властивості Volume, Shuffle, LoopStatus читаються й записуються.
Позиція рахується як остання відома плюс час, що минув, і шле PropertiesChanged та сигнал Seeked.
Bus-name …spotifyd.instance<pid> займається лише при активній сесії й звільняється при розриві.
Застарілі події відсікаються через play_request_id.
Що вивчити: src/dbus_mpris.rs, src/main_loop.rs, docs/src/advanced/{mpris,dbus,hooks}.md.
Пастки: це сервер MPRIS, а твій асистент буде клієнтом, який читає й керує чужими плеєрами. Верифікованого клієнтського референсу в цій вибірці в мене немає. Перевір crate mpris або проксі zbus. HasTrackList=false, тобто черга недоступна. Ліцензію я не перевіряв.
4. Безпечний запуск процесів у Tauri та Rust
tauri-apps/plugins-workspace, plugins/shell (гілка v2) — https://github.com/tauri-apps/plugins-workspace/tree/v2/plugins/shell
Стек: Rust плюс JS-прив'язка.
Ключова фіча: allow-list у capabilities. За документацією v2, дозволена команда описується іменем, програмою та аргументами, а аргумент може мати regex-валідатор. Фронтенд викликає Command.create('run-git-commit', [...]) за іменем, а не рядком команди. Усе поза scope відхиляється. 
tauri
Що вивчити: plugins/shell/src/scope.rs, scope_entry.rs, commands.rs, process/.
Для tool calling Gemini (моя рекомендація, не з репозиторію): не віддавай webview shell узагалі. Зіставляй імена функцій від моделі з enum Rust-обробників із фіксованими аргументами й не склеюй рядки від моделі в команду. Запускай через tokio::process::Command з таймаутом і kill_on_drop.

Інші патерни IPC зі списку вище:

потік-власник плюс канал для не-Send ресурсів (limusic/media.rs);
локальний сокет із токеном або HMAC (firefox-bridge, chromium-bridge);
чого уникати в OpenPet/http_api.rs.
Що я не зміг перевірити
Click-through та drag у OpenPet на Linux/Wayland: це сказано лише в README автора.
Підтримку LM/RDMM у limusic явно (див. вище).
Репозиторії sakura та Meuxe.
Ліцензії spotify-player і spotifyd.
Звідки саме chromium-bridge бере парсер native-messaging кадрів: у дереві є fuzz-таргет nm_frame, значить парсер є в src/packages/core/src/, але файл я не відкривав.

Sources:

OpenPet
littleGuy
chromium-bridge
firefox-bridge
limusic
spotify-player
spotifyd
tauri plugins-workspace, shell
Victauri
Tauri issue #3117 (Wayland always-on-top)
Mozilla Bug 1748169
Music Assistant: YouTube Music
Tauri v2 shell plugin reference