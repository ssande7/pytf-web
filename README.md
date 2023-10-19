# pytf-web

An educational tool for thin film deposition molecular dynamics simulations.

Simulations run in [GROMACS](www.gromacs.org) via [PyThinFilm](github.com/ATB-UQ/PyThinFilm).

2D molecule structures rendered with [smilesDrawer](github.com/reymond-group/smilesDrawer).

3D structures rendered with [Omovi](github.com/andeplane/omovi).

## Installation

1. Install the rust compiler toolchain via [rustup](https://www.rust-lang.org/tools/install)
2. For Rocky linux (tested on version 9) install the packages listed in [`packages_rocky.txt`](packages_rocky.txt). For other OS, equivalent packages should be available.
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

## Running

pytf-web is designed around a single server which handles a job queue, and some
number of workers connected via web sockets.

### Server
Currently, the server runs on port 8080, and expects to be behind something like nginx.
Configuration of port and connection details may become available in future.

To remember user sessions, the server uses Redis, so it requires `redis-server`
to be running.
It also takes an input argument for a list of usernames and passwords,
including the special "worker" user.
It is assumed that user accounts only persist for the duration of a workshop, and
that passwords will be changed between workshops.

To start the server, including Redis:
```
$ ./run_server.sh ${users_file}
```

### Worker
To run a worker node:
```
$ ./run_worker.sh ${server_address} ${worker_key}
```
Currently, workers assume that the server is already available, and will fail to connect
if it is not (even if it comes online later).
This may be changed in future to allow polling of the server if it can't be found.
