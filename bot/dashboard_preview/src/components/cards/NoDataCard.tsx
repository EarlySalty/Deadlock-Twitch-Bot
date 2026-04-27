import { AlertCircle } from "lucide-react";
import type { LucideIcon } from "lucide-react";

interface NoDataCardProps {
  message?: string;
  submessage?: string;
  icon?: LucideIcon;
  className?: string;
}

export function NoDataCard({
  message = "Keine Daten vorhanden",
  submessage = "Daten werden gesammelt sobald du streamst.",
  icon: Icon = AlertCircle,
  className = "",
}: NoDataCardProps) {
  return (
    <div
      className={`flex flex-col items-center justify-center h-48 text-center space-y-2 border border-border rounded-lg bg-card p-6 ${className}`}
    >
      <Icon className="w-8 h-8 text-text-secondary" />
      <p className="text-white font-medium">{message}</p>
      <p className="text-sm text-text-secondary">{submessage}</p>
    </div>
  );
}

export default NoDataCard;
