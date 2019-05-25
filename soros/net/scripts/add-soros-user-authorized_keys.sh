#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

[[ -d /home/soros/.ssh ]] || exit 1

# /soros-authorized_keys contains the public keys for users that should
# automatically be granted access to ALL testnets.
#
# To add an entry into this list:
# 1. Run: ssh-keygen -t ecdsa -N '' -f ~/.ssh/id-soros-testnet
# 2. Inline ~/.ssh/id-soros-testnet.pub below
cat > /soros-authorized_keys <<EOF
ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBFBNwLw0i+rI312gWshojFlNw9NV7WfaKeeUsYADqOvM2o4yrO2pPw+sgW8W+/rPpVyH7zU9WVRgTME8NgFV1Vc=
EOF

sudo -u soros bash -c "
  cat /soros-authorized_keys >> /home/soros/.ssh/authorized_keys
"
