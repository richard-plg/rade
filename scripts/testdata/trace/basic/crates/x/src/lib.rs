#[test]
fn r97_a_resolves_the_base() {}

// fn r97_c_in_a_comment() {}
#[test]
fn r97_cd_similar_id() {
    let s = "fn r97_c_in_a_string";
    let _ = s; // r97_c
}

#[test]
fn r98_b_r97_d_multi_id() {}
