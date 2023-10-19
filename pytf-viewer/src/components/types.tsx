
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
  deposition_velocity: number,
  mixture: Array<MixtureComponent>,
}

