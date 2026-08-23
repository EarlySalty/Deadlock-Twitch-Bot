use tb_chat::lfg_pitch::classify_lfg;

#[test]
fn classify_lfg_erkennt_mitspieler_suche() {
    for text in [
        "suche noch 2 für ranked",
        "wer hat bock zu zocken",
        "lfg",
        "LFG duo?",
        "noch jemand für nen stack",
        "looking for group",
        "jemand zum zocken?",
        "brauche noch einen für die lobby",
    ] {
        assert!(classify_lfg(text), "{text:?}");
    }
}

#[test]
fn classify_lfg_erkennt_anschluss_an_laufende_runde() {
    for text in [
        "ich hau mich dann dazu?",
        "kann ich mit?",
        "darf ich dazu",
        "noch platz frei?",
        "wer zockt noch",
        "will mitspielen",
        "adde mich mal",
    ] {
        assert!(classify_lfg(text), "{text:?}");
    }
}

#[test]
fn classify_lfg_bleibt_bei_anderen_nachrichten_still() {
    for text in [
        "suche einen guten build",
        "wie komme ich rein",
        "",
        "   ",
        "!lfg",
        "was für ein hero ist das",
        "gutes spiel gerade",
        "wie komme ich rein",
        "das ist crazy hahaha",
        "meine freundin ist fort gegangen",
    ] {
        assert!(!classify_lfg(text), "{text:?}");
    }
}
