import { EngineBox } from "@/engine";

export function AnimatedBrandLogo({ expanded }: { expanded: boolean }) {
  return (
    <EngineBox as="span" className={"gm-brand-mark" + (expanded ? " gm-brand-mark--expanded" : "")}>
      <img className="gm-brand-mark__ring" src="/image/git-managar-orange-ring_default-white.png" alt="" draggable={false} />
      <img className="gm-brand-mark__center" src="/image/git-managar-orange-center_default-white.png" alt="" draggable={false} />
    </EngineBox>
  );
}
