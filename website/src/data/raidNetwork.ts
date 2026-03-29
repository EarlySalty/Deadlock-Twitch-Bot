export interface NetworkNode {
  id: string;
  label: string;
  x: number;
  y: number;
}

export interface NetworkEdge {
  from: string;
  to: string;
}

export const DEMO_NODES: NetworkNode[] = [
  { id: "earlysalty", label: "EarlySalty", x: 48, y: 45 },
  { id: "nachtfalke", label: "Nachtfalke", x: 28, y: 30 },
  { id: "pixelraid", label: "PixelRaid", x: 72, y: 28 },
  { id: "saltyviper", label: "SaltyViper", x: 35, y: 65 },
  { id: "icebreaker", label: "IceBreaker", x: 65, y: 62 },
  { id: "sturmjaeger", label: "Sturmjäger", x: 15, y: 50 },
  { id: "flammenherz", label: "Flammenherz", x: 85, y: 45 },
  { id: "schattenkrieger", label: "Schattenkrieger", x: 22, y: 15 },
  { id: "dunkelmond", label: "Dunkelmond", x: 55, y: 15 },
  { id: "blitzgewitter", label: "Blitzgewitter", x: 82, y: 18 },
  { id: "nebeljagd", label: "Nebeljagd", x: 10, y: 75 },
  { id: "klingenwind", label: "Klingenwind", x: 42, y: 82 },
  { id: "drachenatem", label: "Drachenatem", x: 75, y: 80 },
  { id: "frostbiss", label: "Frostbiss", x: 90, y: 68 },
  { id: "silberstreif", label: "Silberstreif", x: 58, y: 90 },
];

export const DEMO_EDGES: NetworkEdge[] = [
  // EarlySalty hub connections (6)
  { from: "earlysalty", to: "nachtfalke" },
  { from: "earlysalty", to: "pixelraid" },
  { from: "earlysalty", to: "saltyviper" },
  { from: "earlysalty", to: "icebreaker" },
  { from: "earlysalty", to: "dunkelmond" },
  { from: "earlysalty", to: "flammenherz" },
  // Outer connections (14)
  { from: "nachtfalke", to: "schattenkrieger" },
  { from: "nachtfalke", to: "sturmjaeger" },
  { from: "pixelraid", to: "blitzgewitter" },
  { from: "pixelraid", to: "dunkelmond" },
  { from: "saltyviper", to: "sturmjaeger" },
  { from: "saltyviper", to: "nebeljagd" },
  { from: "saltyviper", to: "klingenwind" },
  { from: "icebreaker", to: "flammenherz" },
  { from: "icebreaker", to: "drachenatem" },
  { from: "flammenherz", to: "frostbiss" },
  { from: "klingenwind", to: "silberstreif" },
  { from: "drachenatem", to: "silberstreif" },
  { from: "drachenatem", to: "frostbiss" },
  { from: "nebeljagd", to: "sturmjaeger" },
];

export const DEMO_RAID_PATHS: string[][] = [
  ["nachtfalke", "earlysalty", "pixelraid"],
  ["sturmjaeger", "saltyviper", "earlysalty"],
  ["blitzgewitter", "pixelraid", "dunkelmond"],
  ["icebreaker", "drachenatem", "frostbiss"],
  ["nebeljagd", "saltyviper", "klingenwind"],
];
