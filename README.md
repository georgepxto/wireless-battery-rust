# Wireless Battery

Bateria dos dispositivos sem fio direto na barra de tarefas do Windows, com um ícone
para cada tipo: headset, fones (AirPods, Galaxy Buds...), mouse, teclado e controle.

- **Mostrar na barra** (clique no widget): todos os dispositivos ou somente um.
  No modo "todos", desmarque um dispositivo na lista para escondê-lo.
- Texto em **vermelho** com 20% ou menos; **⚡ verde** carregando; **esmaecido** quando o
  dispositivo está dormindo (mostra o último nível conhecido).
- Passe o mouse para ver o nome completo e o estado de cada um.
- Aviso do Windows quando um dispositivo chega a 20%, uma vez por descarga.
- Segue o tema claro/escuro e a escala (DPI) da barra; volta sozinho se o Explorer reiniciar.
- Leitura numa thread separada: HID e controles a cada 20 s, Bluetooth a cada 60 s,
  e na hora quando algo é plugado/removido ou o PC volta da suspensão.

## Fontes de bateria

| Dispositivo | Conexão | Como lê |
|---|---|---|
| Logitech (ex.: PRO X Superlight 2) | receptor Lightspeed/Unifying/Bolt | HID++ (`0x1004`, `0x1000`, `0x1001`, `0x1F20`) |
| MCHOSE V9 Pro | dongle 2.4G / cabo (291D:385D) | protocolo vendor `0x65` |
| AULA F75 | receptor 2.4G (3554:FA09) | protocolo vendor `0x13 0x4A` |
| AULA F75, mouses, teclados BLE | Bluetooth LE | GATT Battery Service |
| AirPods, fones, headsets | Bluetooth clássico | propriedade PnP que o Windows usa em Configurações |
| Controles Xbox e compatíveis | Bluetooth / adaptador Xbox | Windows.Gaming.Input |

## Compilar

```powershell
rustup override set stable-x86_64-pc-windows-gnu   # uma vez, nesta pasta
cargo build --release
```

Executável: `target\release\wireless-battery.exe` (portátil, sem instalação).

O manifesto do Windows (obrigatório para o widget dentro da barra) vem pré-compilado em
`res/resources.o`. Para gerar de novo depois de mudar `res/app.rc`, `res/app.manifest` ou
`res/app.ico`: `x86_64-w64-mingw32-windres res/app.rc -O coff -o res/resources.o`.

## Comandos

```powershell
.\target\release\wireless-battery.exe --demo   # widget com leituras de exemplo
.\target\release\wireless-battery.exe --once   # lista o que foi encontrado e sai
.\target\release\wireless-battery.exe --hid    # lista todas as interfaces HID
```

## Configurações salvas

`HKCU\Software\wireless-battery` (`Display`, `Hidden`, `LowBatteryAlerts`),
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` → `WirelessBattery` e
`HKCU\Software\Classes\AppUserModelId\WirelessBattery` (notificações).

## Limitações

- O posicionamento depende da estrutura interna da barra do Windows 11, que não é API
  pública; uma atualização do Windows pode exigir ajuste. Só a barra do monitor principal.
- Machenike G5 Pro: pelo dongle 2.4G não há bateria (ele se apresenta como controle Xbox com
  fio). Por Bluetooth, no modo PC, aparece como controle Xbox.

## Créditos

Ver [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
