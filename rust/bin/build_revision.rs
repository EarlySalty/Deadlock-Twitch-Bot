#[used]
#[unsafe(link_section = ".twitch_build")]
static BUILD_REVISION: [u8; 47] = {
    let mut bytes = [0; 47];
    let placeholder = b"unstamped";
    let mut index = 0;
    while index < placeholder.len() {
        bytes[index] = placeholder[index];
        index += 1;
    }
    bytes
};

fn print_build_revision() -> bool {
    if std::env::args().nth(1).as_deref() != Some("--build-revision") {
        return false;
    }
    let mut bytes = [0; 47];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = unsafe { std::ptr::read_volatile(BUILD_REVISION.as_ptr().add(index)) };
    }
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    println!("{}", String::from_utf8_lossy(&bytes[..length]));
    true
}
