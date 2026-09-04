import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { avatarUrlFuerGroesse } from "../src/lib/partnerNetwork.ts";

const root = fileURLToPath(new URL("..", import.meta.url));
const scrollFile = `${root}/src/components/ui/ScrollReveal.tsx`;
const netFile = `${root}/src/components/partner-clean/PartnerNetwork.tsx`;
const hookFile = `${root}/src/hooks/useNetworkStreamers.ts`;

test("avatarUrlFuerGroesse schreibt 300x300 auf 70x70 fuer kleine Avatare", () => {
  const url = "https://static-cdn.jtvnw.net/jtv_user_pictures/abc-profile_image-300x300.png";
  assert.equal(
    avatarUrlFuerGroesse(url, 38),
    "https://static-cdn.jtvnw.net/jtv_user_pictures/abc-profile_image-70x70.png",
  );
  assert.equal(
    avatarUrlFuerGroesse(url, 70),
    "https://static-cdn.jtvnw.net/jtv_user_pictures/abc-profile_image-70x70.png",
  );
});

test("avatarUrlFuerGroesse schreibt auf 150x150 fuer mittlere Avatare", () => {
  const url = "https://static-cdn.jtvnw.net/jtv_user_pictures/abc-profile_image-300x300.png";
  assert.equal(
    avatarUrlFuerGroesse(url, 71),
    "https://static-cdn.jtvnw.net/jtv_user_pictures/abc-profile_image-150x150.png",
  );
  assert.equal(
    avatarUrlFuerGroesse(url, 150),
    "https://static-cdn.jtvnw.net/jtv_user_pictures/abc-profile_image-150x150.png",
  );
});

test("avatarUrlFuerGroesse laesst grosse Groessen und fremde Muster unveraendert", () => {
  const url = "https://static-cdn.jtvnw.net/jtv_user_pictures/abc-profile_image-300x300.png";
  assert.equal(avatarUrlFuerGroesse(url, 300), url);
  const fremd = "https://example.com/avatar.png";
  assert.equal(avatarUrlFuerGroesse(fremd, 38), fremd);
  assert.equal(avatarUrlFuerGroesse(undefined, 38), undefined);
});

test("avatarUrlFuerGroesse trifft jpg-Suffix und andere Ausgangsgroessen", () => {
  const jpg = "https://static-cdn.jtvnw.net/jtv_user_pictures/abc-600x600.jpg";
  assert.equal(
    avatarUrlFuerGroesse(jpg, 38),
    "https://static-cdn.jtvnw.net/jtv_user_pictures/abc-70x70.jpg",
  );
});

test("ScrollReveal rendert serverseitig sichtbar und animiert nur unterhalb des Viewports", () => {
  const src = readFileSync(scrollFile, "utf8");
  assert.match(src, /initial=\{false\}/);
  assert.match(src, /useInView/);
  assert.match(src, /useReducedMotion/);
});

test("PartnerNetwork startet Embeds eager und zeigt Vorschaubild als Kachel-Hintergrund", () => {
  const src = readFileSync(netFile, "utf8");
  assert.match(src, /loading="eager"/);
  assert.match(src, /previewImageUrl/);
  assert.match(src, /backgroundImage/);
});

test("useNetworkStreamers startet den Fetch modulweit ausserhalb von useEffect", () => {
  const src = readFileSync(hookFile, "utf8");
  assert.match(src, /netzwerkPromise/);
  const [modulScope, hookScope] = src.split("export function useNetworkStreamers");
  assert.match(modulScope, /fetch\(NETWORK_API\)/);
  assert.doesNotMatch(hookScope, /fetch\(NETWORK_API\)/);
});
