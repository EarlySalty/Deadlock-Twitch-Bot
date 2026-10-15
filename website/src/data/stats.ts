import { landingText } from "@/lib/landingLocale";
export interface Stat {
  label: string;
  value: number;
  suffix: string;
}

export const stats: Stat[] = [
  {
    label: landingText("Streamer"),
    value: 30,
    suffix: "+",
  },
  {
    label: landingText("Module"),
    value: 7,
    suffix: "",
  },
  {
    label: landingText("Analytics-Tabs"),
    value: 13,
    suffix: "",
  },
  {
    label: "Polling",
    value: 15,
    suffix: "s",
  },
  {
    label: "Online",
    value: 24,
    suffix: "/7",
  },
];
