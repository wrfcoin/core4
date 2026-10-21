export function notWired(name: string, reason: string) {
  return {
    isError: true,
    content: [{ type: 'text' as const, text: `${name}: not wired — ${reason} (wrfcoin/core4#3249)` }],
  };
}
