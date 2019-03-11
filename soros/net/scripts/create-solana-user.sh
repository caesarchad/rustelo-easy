#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

adduser bitconch --gecos "" --disabled-password --quiet
adduser bitconch sudo
adduser bitconch adm
echo "bitconch ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers
id bitconch

[[ -r /bitconch-id_ecdsa ]] || exit 1
[[ -r /bitconch-id_ecdsa.pub ]] || exit 1

sudo -u bitconch bash -c "
  mkdir -p /home/bitconch/.ssh/
  cd /home/bitconch/.ssh/
  cp /bitconch-id_ecdsa.pub authorized_keys
  umask 377
  cp /bitconch-id_ecdsa id_ecdsa
  echo \"
    Host *
    BatchMode yes
    IdentityFile ~/.ssh/id_ecdsa
    StrictHostKeyChecking no
  \" > config
"

