//! Headsets e mouses SteelSeries sem fio (VID 1038). Headsets pelo HeadsetControl, mouses pelo
//! rivalcfg. Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), steelseries.cpp.

use super::{clamp100, hex4, map_to_100, read_until, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::{DeviceInfo, HidDevice};

fn write_read(d: &HidDevice, req: &[u8], ms: u32) -> Option<Vec<u8>> {
    d.flush();
    d.write(req).ok()?;
    d.read(ms).filter(|r| !r.is_empty())
}

fn headset(pid: u16, name: &str) -> Status {
    Status::new(format!("steelseries:{}", hex4(pid)), name, Kind::Headset)
}

fn mouse(pid: u16, name: &str) -> Status {
    Status::new(format!("steelseries:{}", hex4(pid)), name, Kind::Mouse)
}

/// Arctis 1 / 1 Xbox / 7X: 06 12 -> [2]==01 desligado, [3] %.
fn arctis1(d: &HidDevice, pid: u16) -> Option<(Status, bool)> {
    let r = write_read(d, &[0x06, 0x12], 1000).filter(|r| r.len() >= 4)?;
    let mut s = headset(pid, "SteelSeries Arctis 1 Wireless");
    s.connected = r[2] != 0x01;
    if s.connected {
        s.percent = clamp100(r[3] as i32);
    }
    Some((s, false))
}

/// Arctis 7 / 7 2019 / Pro 2019: 06 18 -> [2] %.
fn arctis7(d: &HidDevice, pid: u16) -> Option<(Status, bool)> {
    let r = write_read(d, &[0x06, 0x18], 1000).filter(|r| r.len() >= 3)?;
    let mut s = headset(pid, if pid == 0x1252 { "SteelSeries Arctis Pro" } else { "SteelSeries Arctis 7" });
    s.percent = clamp100(r[2] as i32);
    s.connected = r[2] != 0;
    Some((s, false))
}

/// Arctis 9: 00 20 -> [3] bruto (0x64..0x9A), [4]==01 carregando.
fn arctis9(d: &HidDevice, pid: u16) -> Option<(Status, bool)> {
    let r = write_read(d, &[0x00, 0x20], 1000).filter(|r| r.len() >= 5)?;
    let mut s = headset(pid, "SteelSeries Arctis 9");
    s.percent = map_to_100(r[3] as i32, 0x64, 0x9A);
    s.charging = r[4] == 0x01;
    Some((s, false))
}

/// Arctis Pro Wireless: 41 AA -> [0]==02 desligado; 40 AA -> [0] nivel 0..4. A interface 0 nao numera
/// os reports, entao no Windows o comando vai atras de um report id 0.
fn arctis_pro_wireless(d: &HidDevice, pid: u16) -> Option<(Status, bool)> {
    let cmd = |c: u8| write_read(d, &[0x00, c, 0xAA], 1000).or_else(|| write_read(d, &[c, 0xAA], 1000));
    let r = cmd(0x41)?;
    let mut s = headset(pid, "SteelSeries Arctis Pro Wireless");
    s.connected = r[0] != 0x02;
    if s.connected {
        if let Some(r) = cmd(0x40).filter(|r| r[0] <= 4) {
            s.percent = map_to_100(r[0] as i32, 0, 4);
        }
    }
    Some((s, false))
}

/// Arctis Nova Pro Wireless: 06 B0 -> [15] estado (01 desligado, 02 carregando), [6] nivel 0..8.
fn nova_pro_wireless(d: &HidDevice, pid: u16) -> Option<(Status, bool)> {
    let r = write_read(d, &[0x06, 0xB0], 1000).filter(|r| r.len() >= 16)?;
    let mut s = headset(pid, "SteelSeries Arctis Nova Pro Wireless");
    s.connected = r[15] != 0x01;
    s.charging = r[15] == 0x02;
    if s.connected {
        s.percent = map_to_100(r[6] as i32, 0, 8);
    }
    Some((s, false))
}

/// Familia Nova: 00 B0 -> bloco de estado; o formato depende do modelo.
fn nova_family(d: &HidDevice, pid: u16) -> Option<(Status, bool)> {
    let r = write_read(d, &[0x00, 0xB0], 1000).filter(|r| r.len() >= 7)?;
    let mut s;
    if matches!(pid, 0x220E | 0x2212 | 0x2216 | 0x2236) {
        s = headset(pid, "SteelSeries Arctis 7+");
        s.connected = r[1] != 0x01;
        s.percent = map_to_100(r[2] as i32, 0, 4);
        s.charging = r[3] == 0x01;
    } else if matches!(pid, 0x2269 | 0x226D | 0x2232 | 0x2253) {
        s = headset(pid, if matches!(pid, 0x2232 | 0x2253) { "SteelSeries Arctis Nova 5" } else { "SteelSeries Arctis Nova 3P Wireless" });
        s.connected = r[1] != 0x02;
        s.percent = clamp100(r[3] as i32);
        s.charging = r[4] == 0x01;
    } else if pid == 0x230A {
        // Arctis GameBuds: o menor nivel entre os fones fora do estojo.
        s = Status::new(format!("steelseries:{}", hex4(pid)), "SteelSeries Arctis GameBuds", Kind::Earbuds);
        let mut lvl = -1;
        if r[3] == 0x03 {
            lvl = r[5] as i32;
        }
        if r[4] == 0x03 {
            lvl = if lvl < 0 { r[6] as i32 } else { lvl.min(r[6] as i32) };
        }
        s.connected = lvl >= 0;
        s.percent = if lvl >= 0 { clamp100(lvl) } else { -1 };
    } else {
        let discrete = matches!(pid, 0x2202 | 0x2206 | 0x220A | 0x223A | 0x227A | 0x22A4);
        s = headset(pid, "SteelSeries Arctis Nova 7");
        s.connected = r[3] != 0x00;
        s.charging = r[3] == 0x01 || r[3] == 0x02;
        s.percent = if discrete { map_to_100(r[2] as i32, 0, 4) } else { clamp100(r[2] as i32) };
    }
    if !s.connected {
        s.percent = -1;
    }
    Some((s, false))
}

/// Envia `cmd` e devolve o primeiro report que o ecoa. O SteelSeries GG manda os comandos dele na
/// mesma interface e o receptor so responde ao ultimo, entao o pedido e reenviado a cada 100 ms
/// dentro de 1 s. 40 FF = o receptor nao alcanca o mouse (desligado): desiste na hora.
fn query_echo(d: &HidDevice, cmd: u8, min_len: usize) -> Option<Vec<u8>> {
    d.flush();
    let end = Deadline::in_ms(1000);
    while !end.expired() {
        d.write(&[0x00, cmd]).ok()?;
        let resend = Deadline::in_ms(100);
        while !resend.expired() && !end.expired() {
            let Some(r) = d.read(resend.left().min(end.left())) else { break };
            if r.len() >= min_len && r[0] == cmd {
                return Some(r);
            }
            if r.len() >= 2 && r[0] == 0x40 && r[1] == 0xFF {
                return None;
            }
        }
    }
    None
}

/// Familia Aerox / Prime Wireless na interface 3: 92 no cabo, D2 pelo receptor. Dois formatos:
/// - degraus: [1] = (nivel/5 + 1) | carregando<<7; 00 = mouse desligado ou dormindo;
/// - porcentagem (Aerox 3 Wireless Gen 2): [1] 00 bateria, 01 carregando, 02 cheio no cabo;
///   [2] = 0..100 ou FF enquanto nao mediu.
///
/// Mouse desligado fica de fora: o app mantem o ultimo nivel. Cabo e receptor usam a mesma chave
/// (o PID do receptor), entao o mouse aparece uma vez so. (krisgfx9 d74ca56)
///
/// (pid, pid do receptor, nome, cabo, formato em porcentagem)
const AEROX: &[(u16, u16, &str, bool, bool)] = &[
    (0x1838, 0x1838, "SteelSeries Aerox 3 Wireless", false, false),
    (0x183A, 0x1838, "SteelSeries Aerox 3 Wireless", true, false),
    (0x1878, 0x1878, "SteelSeries Aerox 3 Wireless CS2 Dragon Lore Edition", false, false),
    (0x187A, 0x1878, "SteelSeries Aerox 3 Wireless CS2 Dragon Lore Edition", true, false),
    (0x1890, 0x1890, "SteelSeries Aerox 3 Wireless Gen 2", false, true),
    (0x1892, 0x1890, "SteelSeries Aerox 3 Wireless Gen 2", true, true),
    (0x1852, 0x1852, "SteelSeries Aerox 5 Wireless", false, false),
    (0x1854, 0x1852, "SteelSeries Aerox 5 Wireless", true, false),
    (0x1858, 0x1858, "SteelSeries Aerox 9 Wireless", false, false),
    (0x185A, 0x1858, "SteelSeries Aerox 9 Wireless", true, false),
    (0x1840, 0x1840, "SteelSeries Prime Wireless", false, false),
    (0x1842, 0x1840, "SteelSeries Prime Wireless", true, false),
    (0x1860, 0x1860, "SteelSeries Aerox 5 Wireless Diablo IV Edition", false, false),
];

fn aerox(d: &HidDevice, pid: u16) -> Option<(Status, bool)> {
    let &(_, receiver, name, wired, percent) = AEROX.iter().find(|m| m.0 == pid)?;
    let r = query_echo(d, if wired { 0x92 } else { 0xD2 }, if percent { 3 } else { 2 })?;
    if percent && r[1] > 0x02 {
        return None;
    }
    let off = if percent { !wired && r[1] == 0 && r[2] == 0 } else { r[1] & 0x7F == 0 };
    if off {
        return None;
    }
    let mut s = mouse(receiver, name);
    if percent {
        s.charging = r[1] != 0;
        if r[2] <= 100 {
            s.percent = r[2] as i32;
        }
    } else if r[1] & 0x7F <= 21 {
        s.percent = ((r[1] & 0x7F) as i32 - 1) * 5;
        s.charging = r[1] & 0x80 != 0;
    }
    Some((s, wired))
}

/// Rival 3 Wireless (+ Gen 2), Rival 650: AA 01 -> [0] %, [2] carregando.
fn rival(d: &HidDevice, pid: u16) -> Option<(Status, bool)> {
    let r = write_read(d, &[0x00, 0xAA, 0x01], 300).filter(|r| r.len() >= 3)?;
    let r650 = pid == 0x172B || pid == 0x1726;
    let name = if r650 {
        "SteelSeries Rival 650 Wireless"
    } else if pid == 0x1872 {
        "SteelSeries Rival 3 Wireless Gen 2"
    } else {
        "SteelSeries Rival 3 Wireless"
    };
    let mut s = mouse(if r650 { 0x172B } else { pid }, name); // uma entrada so, pelo cabo ou pelo receptor
    s.percent = clamp100(r[0] as i32);
    s.charging = r[2] != 0;
    Some((s, pid == 0x172B))
}

/// Rival Pro (UWB, bateria trocavel): 92 -> 92 00 <% do mouse> <flag> <% da reserva>. (rivalcfg#294)
fn rival_pro(d: &HidDevice, pid: u16) -> Option<(Status, bool)> {
    d.flush();
    d.write(&[0x00, 0x92]).ok()?;
    let pct = read_until(d, 500, |r| (r.len() >= 3 && r[0] == 0x92 && r[1] == 0).then_some(r[2]))?;
    if pct > 100 {
        return None;
    }
    let mut s = mouse(pid, "SteelSeries Rival Pro");
    s.percent = pct as i32;
    Some((s, false))
}

type Poll = fn(&HidDevice, u16) -> Option<(Status, bool)>;

fn ff43(c: &DeviceInfo) -> bool {
    c.usage_page() == 0xFF43 && c.usage() == 0x0202
}
fn ffc0(c: &DeviceInfo) -> bool {
    c.usage_page() == 0xFFC0 && c.usage() == 0x0001
}
fn iface(c: &DeviceInfo, n: i32) -> bool {
    c.interface_number() == n && c.output_len() > 0
}

type Accept = fn(&DeviceInfo) -> bool;

const MODELS: &[(&[u16], Accept, Poll)] = &[
    (&[0x12B3, 0x12B6, 0x12D7], ff43, arctis1),
    (&[0x1260, 0x12AD, 0x1252], ff43, arctis7),
    (&[0x12C2], |c| iface(c, 0), arctis9),
    (&[0x1290], |c| iface(c, 0), arctis_pro_wireless),
    (&[0x12E0, 0x225D], |c| iface(c, 4), nova_pro_wireless),
    (
        &[
            0x220E, 0x2212, 0x2216, 0x2236, 0x2269, 0x226D, 0x2232, 0x2253, 0x230A, 0x2202, 0x22A1, 0x227E, 0x2206, 0x2258, 0x229E, 0x22AD, 0x223A, 0x22A9, 0x227A, 0x22A4,
            0x22A5, 0x220A, 0x22A7,
        ],
        ffc0,
        nova_family,
    ),
    (&[0x1838, 0x183A, 0x1878, 0x187A, 0x1890, 0x1892, 0x1852, 0x1854, 0x1858, 0x185A, 0x1840, 0x1842, 0x1860], |c| iface(c, 3), aerox),
    (&[0x1830, 0x1872], |c| iface(c, 3), rival),
    (&[0x172B, 0x1726], |c| iface(c, 0), rival),
    (&[0x1880], |c| iface(c, 2), rival_pro),
];

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(0x1038) {
        let Some(&(_, accept, poll)) = MODELS.iter().find(|m| m.0.contains(&pid)) else { continue };
        for c in ctx.find(vid, pid, accept) {
            let Some((s, wired)) = ctx.open(&c).and_then(|d| poll(&d, pid)) else { continue };
            // Um Rival 650 carregando no cabo responde pelos dois; vale a leitura do cabo.
            if let Some(i) = ctx.out.iter().position(|o| o.id == s.key) {
                if wired {
                    ctx.out[i] = s.into_observation();
                }
                ctx.claim(vid, pid);
            } else {
                ctx.add(s, vid, pid);
            }
            break;
        }
    }
}

