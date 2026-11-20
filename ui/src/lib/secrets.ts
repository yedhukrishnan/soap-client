// Detects credentials typed literally into a request so the UI can offer to move
// them into a secret variable.

const SECRET_NAMES = "password|passwd|pwd|pass|secret|apikey|api_key|apitoken|token|accesstoken|authtoken|clientsecret|privatekey";

// <prefix:password attr="...">value</prefix:password>
const SECRET_ELEMENT = new RegExp(
  `<((?:[\\w.-]+:)?(${SECRET_NAMES}))(\\s[^>]*)?>([^<]*)</\\1\\s*>`,
  "gi",
);

export interface LiteralSecret {
  /** Element name as written, e.g. "shar:password". */
  tag: string;
  /** Local name, e.g. "password" (used to suggest a variable name). */
  local: string;
  /** Raw element text as written (may contain entities like &amp;). */
  value: string;
  /** The actual secret, with XML entities decoded; this is what gets stored. */
  decoded: string;
  /** The full matched element text, used for replacement. */
  match: string;
}

/** Literal (non-placeholder, non-"?") values inside password-like elements. */
export function findLiteralSecrets(xml: string): LiteralSecret[] {
  const out: LiteralSecret[] = [];
  for (const m of xml.matchAll(SECRET_ELEMENT)) {
    const value = m[4].trim();
    if (!value || value === "?" || value.includes("{{")) continue;
    out.push({ tag: m[1], local: m[2], value: m[4], decoded: decodeXml(m[4]), match: m[0] });
  }
  return out;
}

/** Decode the five predefined XML entities and numeric character references. */
export function decodeXml(s: string): string {
  return s.replace(/&(#x[0-9a-f]+|#\d+|amp|lt|gt|quot|apos);/gi, (whole, e: string) => {
    const lower = e.toLowerCase();
    if (lower.startsWith("#x")) return String.fromCodePoint(parseInt(lower.slice(2), 16));
    if (lower.startsWith("#")) return String.fromCodePoint(parseInt(lower.slice(1), 10));
    return ({ amp: "&", lt: "<", gt: ">", quot: '"', apos: "'" } as Record<string, string>)[lower] ?? whole;
  });
}

/** Replace the value of every occurrence of `s` with `placeholder`. */
export function replaceLiteralSecret(xml: string, s: LiteralSecret, placeholder: string): string {
  const replaced = s.match.replace(`>${s.value}<`, `>${placeholder}<`);
  return xml.split(s.match).join(replaced);
}

export function isLiteral(value: string) {
  return value.length > 0 && !/^\s*\{\{[^}]+\}\}\s*$/.test(value);
}
