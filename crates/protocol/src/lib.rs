//! Sprache auf der Leitung.
pub mod frame;
pub mod nachricht;
pub mod zertifikat;
pub use frame::{lese_rahmen, schreibe_rahmen, MAX_RAHMEN, PROTOKOLL_VERSION};
pub use nachricht::{
    ClientFrame, Fehlercode, Mitgliedsansicht, Raumansicht, ServerFrame, Sprachkopf, CODEC_OPUS,
    CODEC_VERSCHLUESSELT, MAX_SPRACHRAHMEN, SPRACHKOPF_LEN, SPRACHKOPF_VERSION,
};
pub use zertifikat::{
    gleich, gruppiere, normalisiere, pruefe_fingerabdruck, zertifikat_fingerabdruck,
    FINGERABDRUCK_LEN,
};
