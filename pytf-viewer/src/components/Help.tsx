export const Help: React.FC = () => {
  return <div className="MD-param-group">
    <div className="collapsible-content">
      <ol type="1">
        <li>
          <p>In the "Simulation" tab, add molecule types to the film composition.</p>
        </li>
        <li>
          <p>Set the ratio of film components using the number boxes underneath each molecule type.</p>
        </li>
        <li>
          <p>Set the deposition velocity. This controls how quickly the molecules are propelled towards the substrate, and how quickly the film builds up.</p>
        </li>
        <li>
          <p>
            Press the submit button to run the simulation.
            As it runs, the atom trajectories will be streamed back and displayed.
            Keep an eye on the status below the visualisation to see the simulation progress.
          </p>
        </li>
        <li>
          <p>
            Once the simulation finishes, the "Analysis" tab will become available.
            Click "Calculate Roughness" to build a height map of the film with the chosen number of bins and calculate the film thickness and roughness from it.
          </p>
          <p>
            The roughness is calculated as the standard deviation of the bin height, and the thickness is calculated from the mean.
          </p>
          <p>
            Try changing the number of bins used in the calculation. What do you notice?
          </p>
        </li>
        <li>
          <p>
            Set a goal of building either the roughest or smoothest film you can.
            Try different compositions and deposition velocities to see how rough or smooth you can get.
            Use 10 bins each time to keep things consistent, and make sure to record the details your experiments.
          </p>
        </li>
      </ol>
    </div>
  </div>;
}
