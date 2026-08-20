import { useQuery } from '@tanstack/react-query';

import { fetchUplinkMe } from '@/api/uplink';
import { isUplinkTabVisible } from '@/pages/uplinkModel';

/**
 * Ob der Uplink-Eintrag in der Hauptnavigation stehen darf.
 *
 * Die Freigabe kommt vom Uplink selbst (`public_visible`). Ist sie aus oder
 * antwortet der Dienst gar nicht, bleibt der Eintrag dem Admin-Modus
 * vorbehalten. Dieselbe Abfrage benutzt die Uplink-Seite, react-query liefert
 * sie deshalb aus dem Zwischenspeicher.
 */
export function useUplinkVisibility(isAdmin: boolean): boolean {
  const { data } = useQuery({
    queryKey: ['uplink-me'],
    queryFn: fetchUplinkMe,
    retry: false,
    staleTime: 5 * 60 * 1000,
  });
  return isUplinkTabVisible({ publicVisible: data?.public_visible, isAdmin });
}
