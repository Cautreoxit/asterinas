1. apt-get source xorg-server-xorg
2. apply patches in xorg-server folder
3. build xorg-server and install
For example: meson setup builddir --prefix=/usr -Dxorg=true Dxkb_output_dir=/var/lib/xkb -Doptimization=0 -Ddebug=true
ninja -C builddir install

