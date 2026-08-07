const args = process.argv.slice(2);
const argumentLabel =
  args.length === 0
    ? "no arguments"
    : `${args.length} argument${args.length === 1 ? "" : "s"}: ${args.map((argument) => `"${argument}"`).join(" ")}`;
process.stdout.write(`Solaris validation fixture ran with ${argumentLabel}.\n`);
