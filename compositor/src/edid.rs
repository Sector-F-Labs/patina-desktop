//! What a monitor says about itself in its EDID (ADR-0121): who made it,
//! which model, which unit. That is how a monitor is known again whichever
//! port it's plugged into, so the layout chosen for it comes back.

/// The parts of an EDID worth keeping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edid {
    /// The maker's three-letter PNP id ("GSM" is LG).
    pub make: String,
    pub product: u16,
    pub serial: u32,
    /// The model name from its descriptors ("LG HDR WQHD"), if it has one.
    pub name: Option<String>,
    /// The serial number as text, if it has one.
    pub serial_text: Option<String>,
    /// The panel's size in millimetres, as far as the EDID says (it is in
    /// whole centimetres); (0, 0) when it doesn't.
    pub size_mm: (u32, u32),
}

/// Reads the base block of `bytes`; `None` if it isn't an EDID.
pub fn parse(bytes: &[u8]) -> Option<Edid> {
    const HEADER: [u8; 8] = [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0];
    if bytes.len() < 128 || bytes[..8] != HEADER {
        return None;
    }
    let id = u16::from_be_bytes([bytes[8], bytes[9]]);
    let letter = |shift: u16| char::from((((id >> shift) & 31) as u8).wrapping_add(64));
    let make: String = [letter(10), letter(5), letter(0)].iter().collect();
    let product = u16::from_le_bytes([bytes[10], bytes[11]]);
    let serial = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let mut name = None;
    let mut serial_text = None;
    for at in [54, 72, 90, 108] {
        let d = &bytes[at..at + 18];
        // A display descriptor: no pixel clock, then its kind.
        if d[0] != 0 || d[1] != 0 {
            continue;
        }
        let text = || {
            let t: String = d[5..18]
                .iter()
                .take_while(|&&b| b != b'\n')
                .map(|&b| char::from(b))
                .collect();
            let t = t.trim().to_string();
            (!t.is_empty()).then_some(t)
        };
        match d[3] {
            0xfc => name = text(),
            0xff => serial_text = text(),
            _ => {}
        }
    }
    Some(Edid {
        make,
        product,
        serial,
        name,
        serial_text,
        size_mm: (bytes[21] as u32 * 10, bytes[22] as u32 * 10),
    })
}

impl Edid {
    /// A name for this monitor that stays when it's moved to another port:
    /// maker, model and serial. Two monitors that say no serial are the same
    /// to this.
    pub fn id(&self) -> String {
        let serial = match &self.serial_text {
            Some(text) => text.clone(),
            None => format!("{:08X}", self.serial),
        };
        format!("{}-{:04X}-{serial}", self.make, self.product)
    }

    /// Whether it gives a serial at all.
    pub fn has_serial(&self) -> bool {
        self.serial != 0 || self.serial_text.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(make: [u8; 2], product: u16, serial: u32, descriptors: &[(u8, &str)]) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        b[8..10].copy_from_slice(&make);
        b[10..12].copy_from_slice(&product.to_le_bytes());
        b[12..16].copy_from_slice(&serial.to_le_bytes());
        for (i, (kind, text)) in descriptors.iter().enumerate() {
            let at = 54 + 18 * i;
            b[at + 3] = *kind;
            let mut t = text.as_bytes().to_vec();
            t.push(b'\n');
            t.resize(13, b' ');
            b[at + 5..at + 18].copy_from_slice(&t);
        }
        b
    }

    // "GSM" is 0x1E6D.
    const GSM: [u8; 2] = [0x1e, 0x6d];

    #[test]
    fn reads_the_panels_size() {
        let mut b = block(GSM, 1, 1, &[]);
        b[21] = 80;
        b[22] = 34;
        assert_eq!(parse(&b).unwrap().size_mm, (800, 340));
        assert_eq!(parse(&block(GSM, 1, 1, &[])).unwrap().size_mm, (0, 0));
    }

    #[test]
    fn reads_maker_model_and_serial() {
        let edid = parse(&block(GSM, 0x5bbf, 0x0001_02ab, &[(0xfc, "LG HDR WQHD"), (0xff, "812NTAB")]))
            .unwrap();
        assert_eq!(edid.make, "GSM");
        assert_eq!(edid.name.as_deref(), Some("LG HDR WQHD"));
        assert_eq!(edid.id(), "GSM-5BBF-812NTAB");
    }

    #[test]
    fn the_numeric_serial_is_used_without_a_text_one() {
        let edid = parse(&block(GSM, 0x0001, 0x0001_02ab, &[])).unwrap();
        assert_eq!(edid.id(), "GSM-0001-000102AB");
        assert!(edid.has_serial());
        assert!(!parse(&block(GSM, 1, 0, &[])).unwrap().has_serial());
    }

    #[test]
    fn not_an_edid() {
        assert_eq!(parse(&[0; 128]), None);
        assert_eq!(parse(&[0, 0xff]), None);
    }
}
