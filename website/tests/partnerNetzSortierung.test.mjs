import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  istDeadlock,
  impactScore,
  gliederePartner,
  zuschauerSchnitt,
} from "../src/lib/partnerNetwork.ts";

test("zuschauerSchnitt rundet auf ganze Zahlen", () => {
  assert.equal(zuschauerSchnitt(7.666666666666667), "8");
  assert.equal(zuschauerSchnitt(0), "0");
  assert.equal(zuschauerSchnitt(1234.4), "1.234");
});

const root = fileURLToPath(new URL("..", import.meta.url));
const netFile = `${root}/src/components/partner-clean/PartnerNetwork.tsx`;

function partner(login, opts = {}) {
  return {
    login,
    displayName: opts.displayName,
    avatarUrl: opts.avatarUrl,
    isLive: opts.isLive ?? false,
    viewers: opts.viewers ?? 0,
    game: opts.game,
    dlStreams30d: opts.dlStreams30d ?? 0,
    avgViewers30d: opts.avgViewers30d ?? 0,
  };
}

test("istDeadlock erkennt nur das Spiel Deadlock, case-insensitive", () => {
  assert.equal(istDeadlock({ game: "Deadlock" }), true);
  assert.equal(istDeadlock({ game: "deadlock" }), true);
  assert.equal(istDeadlock({ game: "DEADLOCK" }), true);
  assert.equal(istDeadlock({ game: "WARDOGS" }), false);
  assert.equal(istDeadlock({ game: undefined }), false);
  assert.equal(istDeadlock({}), false);
});

test("impactScore ist 50/50 gegen die Maxima und liefert bei Maximum 0 keinen NaN", () => {
  assert.equal(impactScore({ dlStreams30d: 10, avgViewers30d: 100 }, 10, 100), 1);
  assert.equal(impactScore({ dlStreams30d: 5, avgViewers30d: 0 }, 10, 100), 0.25);
  assert.equal(impactScore({ dlStreams30d: 0, avgViewers30d: 50 }, 0, 100), 0.25);
  const wertLeer = impactScore({ dlStreams30d: 3, avgViewers30d: 4 }, 0, 0);
  assert.equal(Number.isNaN(wertLeer), false);
  assert.equal(wertLeer, 0);
});

test("gliederePartner trennt Embeds, weitere Deadlock-Streams und alle Partner", () => {
  const liste = [
    partner("alpha", { isLive: true, game: "Deadlock", viewers: 500, dlStreams30d: 20, avgViewers30d: 300 }),
    partner("bravo", { isLive: true, game: "Deadlock", viewers: 300, dlStreams30d: 10, avgViewers30d: 150 }),
    partner("charlie", { isLive: true, game: "Deadlock", viewers: 800, dlStreams30d: 15, avgViewers30d: 400 }),
    partner("delta", { isLive: true, game: "Deadlock", viewers: 100, dlStreams30d: 5, avgViewers30d: 80 }),
    partner("echo", { isLive: true, game: "Deadlock", viewers: 50, dlStreams30d: 2, avgViewers30d: 40 }),
    partner("foxtrot", { isLive: true, game: "WARDOGS", viewers: 900, dlStreams30d: 8, avgViewers30d: 200 }),
    partner("golf", { isLive: false, game: undefined, viewers: 0, dlStreams30d: 12, avgViewers30d: 90 }),
    partner("hotel", { isLive: false, game: undefined, viewers: 0, dlStreams30d: 1, avgViewers30d: 5 }),
  ];

  const { embeds, weitereDeadlock, allePartner } = gliederePartner(liste);

  assert.deepEqual(embeds.map((s) => s.login), ["charlie", "alpha", "bravo"]);
  assert.deepEqual(weitereDeadlock.map((s) => s.login), ["delta", "echo"]);

  const alleLogins = allePartner.map((s) => s.login);
  assert.ok(!alleLogins.includes("charlie"));
  assert.ok(!alleLogins.includes("alpha"));
  assert.ok(!alleLogins.includes("delta"));
  assert.deepEqual([...alleLogins].sort(), ["foxtrot", "golf", "hotel"]);

  const maxDl = Math.max(...liste.map((s) => s.dlStreams30d));
  const maxAvg = Math.max(...liste.map((s) => s.avgViewers30d));
  for (let i = 1; i < allePartner.length; i++) {
    const vorher = impactScore(allePartner[i - 1], maxDl, maxAvg);
    const jetzt = impactScore(allePartner[i], maxDl, maxAvg);
    assert.ok(vorher >= jetzt, "allePartner ist nicht nach Impact absteigend sortiert");
  }
});

test("gliederePartner bei Gleichstand alphabetisch nach Name", () => {
  const liste = [
    partner("zulu", { isLive: false, dlStreams30d: 4, avgViewers30d: 10 }),
    partner("alfa", { isLive: false, dlStreams30d: 4, avgViewers30d: 10 }),
  ];
  const { allePartner } = gliederePartner(liste);
  assert.deepEqual(allePartner.map((s) => s.login), ["alfa", "zulu"]);
});

test("PartnerNetwork rendert Ausklapp-Kopfzeilen und einen Klapp-Knopf mit aria-expanded", () => {
  const net = readFileSync(netFile, "utf8");
  assert.match(net, /weitere streamen gerade Deadlock/);
  assert.match(net, /Alle \$\{allePartner\.length\} Partner/);
  assert.match(net, /aria-expanded/);
  assert.ok(
    net.includes("Gerade streamt kein Partner Deadlock. Schau später wieder rein."),
    "Leerzustand fuer 0 Deadlock-Live fehlt",
  );
});
