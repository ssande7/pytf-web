
// Sent by server as list of available components

import { Particles } from "omovi"

export type Atom = {
  x: number,
  y: number,
  z: number,
  typ: number,
}

// TODO: include geometry for 3D display
export type MixtureComponentDetailed = {
  res_name: string,
  name: string,
  formula: string,
  smiles: string,
  atoms: Array<Atom>,
}

export type MixtureComponentWith3D = MixtureComponentDetailed & {
  particles: Particles,
}

// Minimal info to pack into config
export type MixtureComponent = {
  res_name: string,
  ratio: number,
}

// Configuration data to send to server
export type PytfConfig = {
  settings: Map<string, any>,
  mixture: Array<MixtureComponent>,
}

// Configuration of an input number for protocol section with optional bounds
export type InputConfig = {
  // Default value to display
  default: number,
  // Optional minimum bound
  min: number | null,
  // Optional maximum bound
  max: number | null,
  // Number of decimal places to show/round to
  dec_places: number | null,
  // Increment between values (enforced during validation)
  increment: number | null,
  // Optional units to display
  display_units: string | null,
  // Name to display. Set to corresponding key if omitted.
  display_name: string | null,
  // Force display to use a number box instead of a slider.
  // Only matters if min, max and dec_places are non-null.
  force_number_box: boolean,
}

