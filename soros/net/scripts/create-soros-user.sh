#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

adduser soros --gecos "" --disabled-password --quiet
adduser soros sudo
adduser soros adm
echo "soros ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers
id soros

[[ -r /soros-id_ecdsa ]] || exit 1
[[ -r /soros-id_ecdsa.pub ]] || exit 1

sudo -u soros bash -c "
  mkdir -p /home/soros/.ssh/
  cd /home/soros/.ssh/
  cp /soros-id_ecdsa.pub authorized_keys
  umask 377
  cp /soros-id_ecdsa id_ecdsa
  echo \"
    Host *
    BatchMode yes
    IdentityFile ~/.ssh/id_ecdsa
    StrictHostKeyChecking no
  \" > config
"

