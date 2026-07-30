use std::net::{Ipv4Addr, SocketAddr};

pub fn parse_pasv_address(response: &str) -> Option<SocketAddr> {
    let start = response.find('(')? + 1;
    let end = response[start..].find(')')? + start;
    let values = response[start..end]
        .split(',')
        .map(str::parse::<u8>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let [a, b, c, d, high, low] = values.as_slice() else {
        return None;
    };
    Some(SocketAddr::from((
        Ipv4Addr::new(*a, *b, *c, *d),
        (u16::from(*high) << 8) | u16::from(*low),
    )))
}

pub fn parse_epsv_port(response: &str) -> Option<u16> {
    let marker = "(|||";
    let start = response.find(marker)? + marker.len();
    let end = response[start..].find("|)")? + start;
    response[start..end].parse().ok()
}

pub fn parse_mdtm(response: &str) -> Option<(i32, u32, u32, u32, u32, u32)> {
    let bytes = response.as_bytes();
    if bytes.len() < 14 {
        return None;
    }
    for start in 0..=bytes.len().saturating_sub(14) {
        let end = start + 14;
        if bytes[start..end].iter().all(u8::is_ascii_digit)
            && (start == 0 || !bytes[start - 1].is_ascii_digit())
            && (end == bytes.len() || !bytes[end].is_ascii_digit())
        {
            let value = std::str::from_utf8(&bytes[start..end]).ok()?;
            return Some((
                value[0..4].parse().ok()?,
                value[4..6].parse().ok()?,
                value[6..8].parse().ok()?,
                value[8..10].parse().ok()?,
                value[10..12].parse().ok()?,
                value[12..14].parse().ok()?,
            ));
        }
    }
    None
}

pub fn parse_size(response: &str) -> Option<usize> {
    response.split_ascii_whitespace().last()?.parse().ok()
}
