import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const css = readFileSync(new URL("../src/app.css", import.meta.url), "utf8");
function declarations(selector: string): Record<string, string> {
  const body = css.slice(css.indexOf(`${selector} {`) + selector.length + 2).split("}")[0].replace(/\/\*[\s\S]*?\*\//g, "");
  return Object.fromEntries([...body.matchAll(/([\w-]+):\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]));
}
function luminance(hex: string) {
  assert.match(hex, /^#[\da-f]{6}$/i);
  const rgb = hex.slice(1).match(/../g)!.map((channel) => {
    const value = parseInt(channel, 16) / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
}
function contrast(a: string, b: string) {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (light + 0.05) / (dark + 0.05);
}
function resolve(value: string, tokens: Record<string, string>): string {
  const variable = /^var\((--[\w-]+)\)$/.exec(value);
  return variable ? resolve(tokens[variable[1]], tokens) : value;
}
for (const theme of ["light", "dark"]) {
  const tokens = { ...declarations(":root"), ...(theme === "dark" ? declarations('[data-theme="dark"]') : {}) };
  test(`${theme} sidebar secondary text meets AA on every library surface`, () => {
    for (const foreground of ["--soft", "--text-muted"]) {
      for (const background of ["--canvas", "--surface-soft", "--surface", "--surface-elev"]) {
        const ratio = contrast(resolve(tokens[foreground], tokens), resolve(tokens[background], tokens));
        assert.ok(ratio >= 4.5, `${foreground} on ${background}: ${ratio.toFixed(2)}:1, requires 4.5:1`);
      }
    }
  });
  test(`${theme} sidebar avatar letter meets AA`, () => {
    const avatar = declarations(".sidebar .account-row .avatar");
    const ratio = contrast(resolve(avatar.color, tokens), resolve(avatar.background, tokens));
    assert.ok(ratio >= 4.5, `avatar contrast: ${ratio.toFixed(2)}:1, requires 4.5:1`);
  });
}
