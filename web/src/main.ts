import init, { parse_xml_content } from "../../crates/wasm/pkg/wasm_mojxml.js";

async function main() {
  await init();
  console.log(parse_xml_content("file.xml", "<root></root>"));
}
main();
