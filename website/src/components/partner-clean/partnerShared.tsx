import { avatarColor, avatarUrlFuerGroesse, initials } from "@/lib/partnerNetwork";

export function Avatar({
  login,
  avatarUrl,
  size = 40,
}: {
  login: string;
  avatarUrl?: string;
  size?: number;
}) {
  return (
    <span
      className="relative flex shrink-0 items-center justify-center overflow-hidden rounded-full font-bold text-black/85"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.4,
        background: avatarColor(login),
      }}
      aria-hidden="true"
    >
      {initials(login)}
      {avatarUrl ? (
        <img
          src={avatarUrlFuerGroesse(avatarUrl, size)}
          alt=""
          loading="lazy"
          className="absolute inset-0 h-full w-full object-cover"
          onError={(e) => {
            e.currentTarget.style.display = "none";
          }}
        />
      ) : null}
    </span>
  );
}

export function LiveBadge() {
  return (
    <span className="flex items-center gap-1.5 rounded bg-[#eb0400] px-2 py-0.5 text-[11px] font-bold uppercase tracking-wider text-white">
      <span className="v2-pulse h-1.5 w-1.5 rounded-full bg-white" />
      Live
    </span>
  );
}
