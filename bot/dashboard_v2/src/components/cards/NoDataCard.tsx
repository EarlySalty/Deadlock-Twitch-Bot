import { AlertCircle } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useT } from "../../context/LanguageContext";

interface NoDataCardProps {
  message?: string;
  submessage?: string;
  icon?: LucideIcon;
  className?: string;
}

export function NoDataCard({
  message,
  submessage,
  icon: Icon = AlertCircle,
  className = "",
}: NoDataCardProps) {
  const t = useT();
  return (
    <div
      className={`flex flex-col items-center justify-center h-48 text-center space-y-2 border border-border rounded-lg bg-card p-6 ${className}`}
    >
      <Icon className="w-8 h-8 text-text-secondary" />
      <p className="text-white font-medium">{message ?? t("Keine Daten vorhanden")}</p>
      <p className="text-sm text-text-secondary">{submessage ?? t("Daten werden gesammelt sobald du streamst.")}</p>
    </div>
  );
}

export default NoDataCard;
