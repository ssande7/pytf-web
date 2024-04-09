import React, { useEffect, useRef, useState } from "react";
import { SvgDrawer, parse } from "smiles-drawer";

interface ISmilesImg {
  smiles: string,
  options: any,
};

const SmilesImg: React.FC<ISmilesImg> = ({ smiles, options }) => {
  const canvas_ref = useRef<SVGSVGElement | null>(null);
  const [sd, setSD] = useState<SvgDrawer | null>(null)
  useEffect(() => {
    setSD(new SvgDrawer({
      compactDrawing: false,
      explicitHydrogens: true,
      terminalCarbons: true,
      ...options
    }));
  }, [options])

  useEffect(() => {
    if (canvas_ref.current === null || sd === null) return;
    parse(smiles, function (tree: any) {
      sd.draw(tree, canvas_ref.current, "light");
    });
  }, [smiles, sd]);

  return (
    <svg id="smiles-canvas" ref={canvas_ref}/>
  );
}

export default SmilesImg;
