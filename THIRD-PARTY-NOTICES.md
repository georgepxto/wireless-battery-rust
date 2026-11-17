# Código e protocolos de terceiros

## renanleonel/windows-taskbar-battery (MIT)
https://github.com/renanleonel/windows-taskbar-battery

`src/logitech/` (HID++), `src/bluetooth.rs`, `src/xbox.rs`, `src/device.rs`, `src/widget.rs`,
`src/render.rs`, `src/notify.rs`, `src/settings.rs`, `src/playstation.rs`, `src/headsetcontrol.rs`
e o manifesto em `res/` vêm desse projeto,
com adaptações (textos em português, modo "somente um", estado "carregando sem porcentagem",
fonte de ícones do Windows 10).

```
MIT License

Copyright (c) 2026 Renan Leonel

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## jmjp/mchose-hub-native (MIT, Copyright (c) 2026 João (jmjp))
https://github.com/jmjp/mchose-hub-native

Protocolo de bateria do MCHOSE V9 Pro usado em `src/mchose.rs`.

## ushineko/sanshoku (MIT)
https://github.com/ushineko/sanshoku/issues/36

Protocolo de bateria do receptor 2.4G da AULA F75 usado em `src/aula.rs`.
