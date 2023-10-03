
// Sent by server as list of available components
// TODO: include image as well?
export type MixtureComponentDetailed = {
  res_name: string,
  name: string,
  formula: string,
  natoms: number,
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

