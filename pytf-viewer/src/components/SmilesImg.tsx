import React, { useEffect, useRef } from "react";
import { SvgDrawer, parse } from "smiles-drawer";

interface ISmilesImg {
  smiles: string,
  options: any,
};

const SmilesImg: React.FC<ISmilesImg> = ({ smiles, options }) => {
  const canvas_ref = useRef<SVGSVGElement | null>(null);

  let sd = new SvgDrawer({
    compactDrawing: false,
    explicitHydrogens: true,
    terminalCarbons: true,
    ...options
  });
  useEffect(() => {
    if (canvas_ref.current === null) return;
    parse(smiles, function (tree: any) {
      sd.draw(tree, canvas_ref.current, "light");
    });
  }, [smiles]);

  return (
    <svg id="smiles-canvas" ref={canvas_ref} style={{display: 'inline-block', verticalAlign: 'middle'}}/>
  );
}

export default SmilesImg;
