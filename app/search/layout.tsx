import { PanoramaSearchRouteProvider } from "@/components/PanoramaSearchRoute";

export default function SearchLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return <PanoramaSearchRouteProvider>{children}</PanoramaSearchRouteProvider>;
}
