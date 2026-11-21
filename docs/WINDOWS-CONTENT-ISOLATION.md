# Windows content isolation: проект для согласования

Статус: **BLOCKED / не реализовано**. Этот документ не включает sandbox и не
доказывает безопасность текущего браузера. До реализации и негативных runtime
проверок Windows-сборка остаётся developer/experimental.

## Подтверждённое исходное состояние

- `components/constellation/sandboxing.rs`: Windows создаёт обычный дочерний
  процесс и возвращает `Process::Unsandboxed`. Windows sandbox profile вызывает
  `exit(1)`, а не устанавливает ограничения.
- `ports/servoshell/prefs.rs`: multiprocess и sandbox являются отдельными CLI
  настройками. Feature при компиляции не включает их при запуске.
- `Start-Browser.cmd` и ярлыки установщика не включают multiprocess.
- `UnprivilegedContent` содержит opts/preferences и IPC endpoints. Их текущие
  сериализуемые типы нельзя автоматически считать безопасным broker API.
- Фактический runtime multiprocess/crash containment ещё не проверен в этом
  этапе. Отдельный PID сам по себе не является ограничением прав ОС.

## Границы доверия и активы

Предполагается компрометация content process через JS, DOM, шрифты или декодер.
Нужно защитить профиль, Windows credentials/DPAPI, прочие файлы пользователя,
другие сайты/процессы, сетевые ключи и управляющие возможности browser chrome.
Broker остаётся привилегированной доверенной частью; неизвестный IPC, дескриптор
или origin от content — недоверенные данные, даже если тип Rust корректен.

## Предлагаемая схема

1. **Browser/broker** владеет профилем, vault, DPAPI и пользовательскими диалогами.
   Content не получает путь профиля, полный preferences snapshot, ключи подписки,
   credential vault, handles к broker process или произвольный filesystem API.
2. **Content** запускается с AppContainer token без сетевых capabilities и без
   предоставления доступа к пользовательскому профилю. Нужные read-only ресурсы
   и шрифты — минимальная allowlist либо broker-сервис. Реальную совместимость
   Servo/SpiderMonkey, shared memory, D3D и IPC требуется доказать прототипом.
3. **Network/storage services** принимают только проверенные операции, связанные
   с document/site partition. Content не получает управляющий сокет sing-box/Xray
   или прямой доступ к сетевым секретам. Брокер не предоставляет raw CONNECT к
   произвольной цели в обход CORS/cookie/permission политики.
4. **GPU/media** отдельно исследовать как дополнительные недоверенные процессы:
   текущий вывод кадров или декодирование в broker оставляют соответствующий
   attack surface вне sandbox. Изоляция только script не закрывает этот риск.

## Механизмы Windows и ограничения

- Предпочтительный кандидат — AppContainer через
  `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`. Не выдавать broad network
  capabilities или blanket profile ACL ради запуска. Проверить LPAC отдельно,
  не считать его автоматически совместимым с этой архитектурой.
- Restricted token удаляет privileges и ограничивает SIDs, но сам по себе не
  гарантирует нужную файловую/сетевую изоляцию. Возможен исследовательский этап,
  **не безопасный fallback** при отказе AppContainer. Microsoft отдельно
  предупреждает о взаимодействии restricted processes с общим desktop.
- Старт через `STARTUPINFOEX` с явным `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`:
  наследуются только выделенные IPC/shared-memory handles с минимальными правами.
  Отсутствуют writable handles к профилю, токенам и процессам; стандартные потоки
  не наследуют произвольные родительские handles. Перечень проверяется runtime.
- Job Object используется для lifetime/quotas/запрета breakaway, **не называется
  sandbox**. Ограничения child-process и mitigations — дополнительные меры.
- Нельзя вслепую запретить dynamic code при включённом JS JIT или все Win32k
  вызовы при текущем графическом стеке. Сначала определить совместимый набор,
  затем доказать применённые mitigations runtime; ничего не отключать молча.

## Broker IPC: обязательные свойства

- Версионированные сообщения с жёсткими пределами размера/очереди/handles,
  проверка полномочий по broker-owned document/site ID и generation.
- Origin, top-level origin, frame ownership и permission grant не принимаются
  на веру из payload. Навигация отзывает прежние grants и pending requests.
- Не передавать в IPC пути для открытия/записи от content. Для upload/download —
  непрозрачные scoped capabilities после явного выбора пользователя; канонизация,
  запрет traversal/reparse-point обходов и stale handle reuse.
- Cookie/storage операции используют проверенный partition/request context;
  пароль заполняется только после явного действия и проверки живого документа.
  «Расшифруй DPAPI blob» или «прочитай произвольный файл» не являются IPC командами.
- Отказ настройки токена/ACL/handles/IPC завершает создание content. Никакого
  незаметного старта с полными правами. Ошибки не включают секретные payload.

## Этапы и критерий приёмки

1. Согласовать границы процессов и отдельный объём работ. Без смены движка.
2. В headless synthetic harness подтвердить фактические PID/режимы; вызвать
   контролируемое завершение **своего** content child. Broker/другая вкладка
   должны пережить отказ. Это только crash containment, не OS sandbox proof.
3. Отдельный restricted/AppContainer probe без браузерного профиля: запретить
   чтение/запись synthetic sentinel вне разрешённой области, чтение чужого
   DPAPI ciphertext, доступ к registry/другому процессу, создание child и прямую
   loopback/Internet сеть. Отрицательный тест обязан пробовать реальную операцию.
4. Интеграция минимальной страницы: IPC/shared memory, шрифты, JIT, графика,
   lifecycle и cleanup. Проверить malformed IPC, oversized payload, лишние handles,
   spoofed origin, stale generation, broker disconnect и resource exhaustion.
5. Проверить Windows 10/11 на чистых test instances и независимо от headless
   проверить GUI/IME/DPI/media. Только после независимого security review и
   негативных проверок можно менять статус на sandboxed.

## Первичные источники

Проверены 9 ноября 2026, описывают механизмы ОС, не готовность Servo:

- [AppContainer isolation](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation)
- [Restricted tokens](https://learn.microsoft.com/en-us/windows/win32/secauthz/restricted-tokens)
- [UpdateProcThreadAttribute: handle list, capabilities, mitigations, child policy](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)
- [Наследование pipe handles](https://learn.microsoft.com/en-us/windows/win32/procthread/creating-a-child-process-with-redirected-input-and-output)
