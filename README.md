# Wireless Battery

Ícone na bandeja do Windows com a bateria dos dispositivos sem fio.

- O ícone mostra o **menor nível** entre os dispositivos conectados (vermelho em 20% ou menos).
- Passe o mouse para ver todos; clique para o menu (lista, atualizar, iniciar com o Windows, sair).
- Aviso do Windows quando um dispositivo chega a 20%, uma vez por descarga.
- Leitura numa thread separada: HID e controles a cada 20 s, Bluetooth a cada 60 s,
  e na hora quando algo é plugado/removido ou o PC volta da suspensão.

## Fontes

| Dispositivo | Conexão | Como lê |
|---|---|---|
| Logitech (ex.: PRO X Superlight 2) | receptor Lightspeed/Unifying/Bolt | HID++ (`0x1004`, `0x1000`, `0x1001`, `0x1F20`) |
| MCHOSE V9 Pro | dongle 2.4G / cabo (291D:385D) | protocolo vendor `0x65` |
| AULA F75 | receptor 2.4G (3554:FA09) | protocolo vendor `0x13 0x4A` |
| AULA F75, mouses, teclados BLE | Bluetooth LE | GATT Battery Service |
| Fones, headsets | Bluetooth clássico | propriedade PnP que o Windows usa em Configurações |
| Controles Xbox e compatíveis | Bluetooth / adaptador Xbox | Windows.Gaming.Input |

## Compilar

```powershell
rustup override set stable-x86_64-pc-windows-gnu   # uma vez, nesta pasta
cargo build --release
```

Executável: `target\release\wireless-battery.exe`.

## Diagnóstico

```powershell
.\target\release\wireless-battery.exe --once   # lista o que foi encontrado e sai
.\target\release\wireless-battery.exe --hid    # lista todas as interfaces HID (VID:PID, usage page)
```

## Créditos

Ver [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
