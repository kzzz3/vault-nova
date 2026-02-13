use anyhow::{Result, anyhow, bail};
use rand::seq::SliceRandom;

const LOWERCASE: &[u8] = b"abcdefghijkmnopqrstuvwxyz";
const UPPERCASE: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
const DIGITS: &[u8] = b"23456789";
const SYMBOLS: &[u8] = b"!@#$%^&*()-_=+[]{}<>?";

pub fn generate_password(
    length: usize,
    include_numbers: bool,
    include_symbols: bool,
) -> Result<String> {
    let mut character_groups: Vec<&[u8]> = vec![LOWERCASE, UPPERCASE];

    if include_numbers {
        character_groups.push(DIGITS);
    }
    if include_symbols {
        character_groups.push(SYMBOLS);
    }

    if length < character_groups.len() {
        bail!(
            "length must be at least {} when using the selected character groups",
            character_groups.len()
        )
    }

    let mut rng = rand::thread_rng();
    let mut password_chars: Vec<char> = Vec::with_capacity(length);

    for group in &character_groups {
        password_chars.push(random_char(group, &mut rng)?);
    }

    while password_chars.len() < length {
        let group = character_groups
            .choose(&mut rng)
            .copied()
            .ok_or_else(|| anyhow!("no character groups available for generation"))?;
        password_chars.push(random_char(group, &mut rng)?);
    }

    password_chars.shuffle(&mut rng);
    Ok(password_chars.into_iter().collect())
}

fn random_char(group: &[u8], rng: &mut impl rand::Rng) -> Result<char> {
    let byte = group
        .choose(rng)
        .copied()
        .ok_or_else(|| anyhow!("character group cannot be empty"))?;
    Ok(byte as char)
}

#[cfg(test)]
mod tests {
    use super::generate_password;

    #[test]
    fn generate_password_uses_requested_length() {
        let password = generate_password(24, true, true).expect("generation should succeed");
        assert_eq!(password.len(), 24);
    }

    #[test]
    fn generate_password_fails_for_short_length() {
        let result = generate_password(2, true, true);
        assert!(result.is_err());
    }

    #[test]
    fn generate_password_without_symbols_has_no_symbol_characters() {
        let password = generate_password(40, true, false).expect("generation should succeed");
        assert!(
            !password
                .chars()
                .any(|ch| "!@#$%^&*()-_=+[]{}<>?".contains(ch))
        );
    }
}
