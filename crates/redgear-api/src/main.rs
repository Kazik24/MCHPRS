fn main() -> std::io::Result<()> {
    let address = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8765".into());
    let socket: std::net::SocketAddrV4 = address.parse().map_err(std::io::Error::other)?;
    if !socket.ip().is_loopback() {
        return Err(std::io::Error::other("loopback IPv4 required"));
    }
    mroww_redgear_api::run(&address)
}
