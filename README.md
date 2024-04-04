# pytf-web

An educational tool for thin film deposition molecular dynamics simulations.


https://github.com/ssande7/pytf-web/assets/1731652/48cca024-ddbc-465a-b31e-6fa428e727c4


Simulations run in [GROMACS](https://www.gromacs.org) via [PyThinFilm](https://github.com/ATB-UQ/PyThinFilm).

2D molecule structures rendered with [smilesDrawer](https://github.com/reymond-group/smilesDrawer).

3D structures rendered with [Omovi](https://github.com/andeplane/omovi).

---

## Installation

1. Install the rust compiler toolchain via [rustup](https://www.rust-lang.org/tools/install)
2. For Rocky linux (tested on version 9) install the packages listed in
   [`packages_rocky.txt`](packages_rocky.txt). For other OS, equivalent
   packages should be available.
3. Set up the python virtual environment with PyThinFilm:
```
$ ./setup_pyenv.sh
```
4. Compile the server and worker code, and the web interface:
```
$ ./build.sh
```
5. Compile and install GROMACS. For example:
```
$ curl https://ftp.gromacs.org/gromacs/gromacs-2023.2.tar.gz --output gromacs-2023.2.tar.gz
$ tar -xvf gromacs-2023.2.tar.gz
$ cd gromacs-2023.2
$ mkdir build && cd build
$ cmake .. -DGMX_BUILD_OWN_FFTW=ON
$ make -j$(nproc)
$ sudo make install
```
6. Configure nginx (optional):
```
    root            /path/to/pytf-web/pytf-viewer/public;
    location / {
        proxy_pass http://127.0.0.1:8080;
    }

    location /socket {
        proxy_pass http://127.0.0.1:8080/socket;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 300s;
    }
```

---

## Running

pytf-web is designed around a single server which handles a job queue, and some
number of workers connected via web sockets which run the simulations. Users
submit configurations to the server, which are then distributed to workers. To
avoid unnecessary duplication of work, equivalent configurations are combined
into a single job, and results are cached in server memory and archived to disk
after some delay (from which they can later be retrieved). Partially complete
jobs are also cached and archived when possible, and can be resumed upon
resubmission of the same configuration.

The default molecular dynamics force field is the Automated Topology Builder
([ATB](https://atb.uq.edu.au)) force field, although in principle any
GROMACS-compatible force field can be used. With the default force field,
molecule options for deposition can be added by adding the corresponding .pdb
and .itp files (sourced from the ATB) to the `resources/molecules` directory,
and then adding an entry to the [`molecules.json`](resources/molecules.json) file.
Make sure the `res_name` field matches the residue name of the molecule in the
.pdb file, and that the files are named `${res_name}.pdb` and
`${res_name}.itp`, otherwise they will not be found. The .pdb and .itp files must
be present on all worker instances, and the .pdb files are also required by the
server to extract 3D molecule structure for display.
For display of the 2D structure, the required SMILES string in
[`molecules.json`](resources/molecules.json) can also be found on the ATB, or
from various other sources.
See the default molecules provided in this repository for examples.

The [PyThinFilm configuration](https://atb-uq.github.io/PyThinFilm) can be modified by
editing [`base_config.yml`](resources/base_config.yml). Note that some options
are filled by the web server and are therefore deliberately omitted.
Omitted values (with the exceptions of `name`, `work_directory` and `mixture`) should
appear in [`input_config.yml`](resources/input_config.yml). These can be literals,
user-configurable fields (displayed in the Protocol section of the web page), or
formulas to be calculated from other fields in the file. In the latter case, formulas
follow the syntax specified by the [evalexpr](https://docs.rs/evalexpr/latest/evalexpr/)
crate (a simple example is included in [`input_config.yml`](resources/input_config.yml)).

### Server
By default, the server runs on port 8080, and expects to be behind something like nginx.
Run `pytf-server` with the `-h` or `--help` flag to see configuration options.

The server requires access to the `resources` directory (structured as the
default provided in this repository) and to an `archive` directory for storing
old inactive jobs. Both of these can be configured with the `--resources` and
`--archive` flags, and the `archive` directory will be created if it does not exist.
If it does exist, existing archived jobs within it will be used when possible.

For login details, a file containing comma-separated (with no whitespace)
usernames and argon2 password hashes, one per line, is required via the `--users` flag.
This should include an entry for the special "worker" user, and can be generated with
the provided `pytf-hash-users` tool from a similarly formatted file with
plaintext passwords (see [`test_users.csv`](test_users.csv) for an example):
```
$ cargo run --release pytf-hash-users test_users.csv -o test_users.hashed
```

To remember user sessions, the server uses Redis, so it requires `redis-server`
to be running. The address and port of the Redis server can be configured on
the command line via the `--redis-ip` and `--redis-port` arguments, although the
defaults should work if a standard Redis configuration is used.

To start the server, including Redis:
```
$ ./run_server.sh ${users_file}
```

### Worker
To run a worker node:
```
$ ./run_worker.sh ${server_ip} ${worker_key}
```
If the server is unavailable when a worker is started, or becomes unavailable
later, workers will periodically attempt to reconnect.

Note that if the server is not accessible via the standard HTTP port,
then the port must be specified along with the address
(e.g. `./run_worker.sh '127.0.0.1:8080' P@ssw0rd!`). If the server is proxied to
the HTTP port by nginx (as in the example configuration above), then it is
sufficient to just use the IP address of the nginx server.

