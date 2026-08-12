const SECRET_PATTERNS: readonly (readonly [RegExp, string])[] = [
	[/\b(sk|rk|pk)-[A-Za-z0-9_-]{8,}/g, "$1-REDACTED"],
	[/\bgh[pousr]_[A-Za-z0-9]{16,}/g, "gh_REDACTED"],
	[/\bxox[baprs]-[A-Za-z0-9-]{10,}/g, "xox-REDACTED"],
	[/\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}(?:\.[A-Za-z0-9_-]+)?/g, "REDACTED-JWT"],
	[/\b(Bearer|Basic|Token)\s+[A-Za-z0-9._~+/=-]{8,}/gi, "$1 REDACTED"],
	[
		/([?&](?:access_token|refresh_token|id_token|api[_-]?key|apikey|token|secret|signature|sig|code|password)=)[^&\s"'<>]+/gi,
		"$1REDACTED",
	],
	[
		/((?:"|')?(?:authorization|api[_-]?key|apikey|access[_-]?token|refresh[_-]?token|id[_-]?token|client[_-]?secret|secret|password|passwd)(?:"|')?\s*[:=]\s*(?:"|')?)[^\s"',;}\]]{4,}/gi,
		"$1REDACTED",
	],
];

export function redactObviousSecrets(value: string): string {
	let redacted = value;
	for (const [pattern, replacement] of SECRET_PATTERNS) {
		pattern.lastIndex = 0;
		redacted = redacted.replace(pattern, replacement);
	}
	return redacted;
}

export function privacyFilteredText(
	value: string,
	options: { readonly home?: string; readonly maxBytes: number },
): string {
	if (!Number.isSafeInteger(options.maxBytes) || options.maxBytes < 3) {
		throw new RangeError("privacy-filter byte bound must be a safe integer of at least 3");
	}
	let filtered = redactObviousSecrets(value)
		.replace(/\r\n?/g, "\n")
		.replace(/\t/g, "  ")
		.replace(/[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/g, "�");
	const home = options.home?.replace(/\/+$/u, "");
	if (home) filtered = filtered.replaceAll(home, "~");
	const encoded = Buffer.from(filtered, "utf8");
	if (encoded.byteLength <= options.maxBytes) return filtered;
	const marker = Buffer.from("…", "utf8");
	const kept = encoded.subarray(0, Math.max(0, options.maxBytes - marker.byteLength))
		.toString("utf8")
		.replace(/\ufffd$/u, "");
	return `${kept}…`;
}
