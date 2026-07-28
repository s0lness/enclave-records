use crate::state::{Art, ART_LEN};
use crate::AppSW;
use ledger_device_sdk::io::{Command, CommandResponse};

/// SET_ART: p1 = slot (0 master, 1 pressing), data = offset(u16 LE) || chunk.
/// Cover art is public data (it travels with the album), so no UI gate. What
/// makes it trustworthy is the sleeve hash signed into the album certificate
/// at cut time: the device renders a slot's art only when its hash matches
/// that record's own certificate.
pub fn handler_set_art(command: Command<'_>, slot: u8) -> Result<CommandResponse<'_>, AppSW> {
    let data = command.get_data();
    if data.len() < 3 {
        return Err(AppSW::WrongApduLength);
    }
    let offset = u16::from_le_bytes([data[0], data[1]]) as usize;
    Art::write_chunk(slot as usize, offset, &data[2..])?;
    let response = command.into_response();
    Ok(response)
}

/// GET_ART: read back a chunk (p1 = chunk index, p2 = slot) so the relay can
/// carry a record's art to the receiving device during a press.
pub fn handler_get_art(
    command: Command<'_>,
    chunk: u8,
    slot: u8,
) -> Result<CommandResponse<'_>, AppSW> {
    const CHUNK: usize = crate::state::ART_CHUNK;
    let offset = chunk as usize * CHUNK;
    if offset >= ART_LEN {
        return Err(AppSW::WrongP1P2);
    }
    let end = (offset + CHUNK).min(ART_LEN);
    let art = Art::get(slot as usize);
    let mut response = command.into_response();
    response.append(&art[offset..end])?;
    Ok(response)
}
