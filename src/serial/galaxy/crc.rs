use thiserror::Error;

pub trait GalaxyCRC {
    fn galaxy_crc(&self) -> u8;
}

#[derive(Debug, Error, PartialEq)]
#[error("expected CRC: {0:02X}")]
pub struct GalaxyCRCCheckError(u8);

type GalaxyCRCCheckResult = Result<(), GalaxyCRCCheckError>;

pub trait CheckGalaxyCRC {
    fn check_galaxy_crc(&self) -> GalaxyCRCCheckResult;
}

impl GalaxyCRC for Vec<u8> {
    fn galaxy_crc(&self) -> u8 {
        (&self[..]).galaxy_crc()
    }
}

impl CheckGalaxyCRC for Vec<u8> {
    fn check_galaxy_crc(&self) -> GalaxyCRCCheckResult {
        self[..].check_galaxy_crc()
    }
}

impl GalaxyCRC for [u8] {
    fn galaxy_crc(&self) -> u8 {
        galaxy_crc(self)
    }
}

impl CheckGalaxyCRC for [u8] {
    fn check_galaxy_crc(&self) -> GalaxyCRCCheckResult {
        assert!(self.len() >= 1, "message has no embedded CRC");

        let msg_crc: u8 = self[self.len() - 1];
        let expect_crc = galaxy_crc(&self[0..self.len() - 1]);

        if msg_crc == expect_crc {
            Ok(())
        } else {
            Err(GalaxyCRCCheckError(expect_crc))
        }
    }
}

pub fn galaxy_crc(msg: &[u8]) -> u8 {
    galaxy_crc_vectored(&[msg])
}

pub fn galaxy_crc_vectored(vecs: &[&[u8]]) -> u8 {
    let mut acc = 0xAAu16;

    for &vec in vecs {
        for &c in vec {
            acc += c as u16;
        }

        while acc > 0xFF {
            acc = (acc & 0xFF) + (acc >> 8);
        }
    }

    acc as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_galaxy_crc() {
        let crc = galaxy_crc(vec![0x10, 0x00, 0x0E].as_slice());
        let expect = 0xC8;

        assert_eq!(
            crc, expect,
            "CRC failure: expected={:02X} got={:02X}",
            crc, expect
        );
    }

    #[test]
    fn test_vector() {
        let crc = vec![0x10, 0x00, 0x0E].galaxy_crc();
        assert_eq!(crc, 0xC8);
    }

    #[test]
    fn test_check_vector() {
        assert!(vec![0x10, 0x00, 0x0E, 0xC8].check_galaxy_crc().is_ok());
        assert!(vec![0x10, 0x00, 0x0E, 0xAB].check_galaxy_crc().is_err());
    }
}
