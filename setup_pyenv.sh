#!/bin/bash
SYSTEM_PY=${PYTHON3:-python3}
${SYSTEM_PY} -m venv pyenv
source pyenv/bin/activate
python -m pip install git+https://github.com/ATB-UQ/PyThinFilm@c77ad51
