cargo build --release
cargo zigbuild --release --target x86_64-pc-windows-gnullvm

CMD="cp \"$PWD/target/release/img_tool\" \"$PWD/target/x86_64-pc-windows-gnullvm/release/img_tool.exe\" ."
echo "$CMD"
echo "$CMD" | pbcopy
