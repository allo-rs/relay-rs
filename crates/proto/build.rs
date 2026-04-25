// 使用 protox（纯 Rust protoc 替代）避免对系统 protoc 的依赖
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file_descriptors = protox::compile(
        ["proto/relay.proto"],
        ["proto/"],
    )?;

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_fds(file_descriptors)?;

    println!("cargo:rerun-if-changed=proto/relay.proto");
    Ok(())
}
