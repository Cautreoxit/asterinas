# SPDX-License-Identifier: MPL-2.0

BINARY_CACHE_DIR := /opt/linux_binary_cache
VDSO_DIR := ../target
VDSO_LIB := $(VDSO_DIR)/vdso64.so
MKFILE_PATH := $(abspath $(lastword $(MAKEFILE_LIST)))
CUR_DIR := $(patsubst %/,%,$(dir $(MKFILE_PATH)))
ATOMIC_WGET := $(CUR_DIR)/../tools/atomic_wget.sh
BUILD_DIR := $(CUR_DIR)/build
INITRAMFS := $(BUILD_DIR)/initramfs
INITRAMFS_FILELIST := $(BUILD_DIR)/initramfs.filelist
INITRAMFS_SKIP_GZIP ?= 0
ifeq ($(INITRAMFS_SKIP_GZIP),1)
INITRAMFS_IMAGE := $(BUILD_DIR)/initramfs.cpio
else
INITRAMFS_IMAGE := $(BUILD_DIR)/initramfs.cpio.gz
endif
EXT2_IMAGE := $(BUILD_DIR)/ext2.img
EXFAT_IMAGE := $(BUILD_DIR)/exfat.img
INITRAMFS_EMPTY_DIRS := \
	$(INITRAMFS)/root \
	$(INITRAMFS)/tmp \
	$(INITRAMFS)/opt \
	$(INITRAMFS)/proc \
	$(INITRAMFS)/dev \
	$(INITRAMFS)/sys \
	$(INITRAMFS)/ext2 \
	$(INITRAMFS)/exfat \
	$(INITRAMFS)/var/log

INITRAMFS_ALL_DIRS := \
	$(INITRAMFS)/etc \
	$(INITRAMFS)/run \
	$(INITRAMFS)/var/run \
	$(INITRAMFS)/run/user/1000 \
	$(INITRAMFS)/lib/x86_64-linux-gnu \
	$(INITRAMFS)/lib64 \
	$(INITRAMFS)/bin \
	$(INITRAMFS)/sbin \
	$(INITRAMFS)/usr/bin \
	$(INITRAMFS)/usr/local \
	$(INITRAMFS)/usr/share \
	$(INITRAMFS)/test \
	$(INITRAMFS)/benchmark \
	$(INITRAMFS)/etc/X11 \
	$(INITRAMFS)/etc/dbus-1 \
	$(INITRAMFS)/usr/lib \
	$(INITRAMFS)/usr/lib/x86_64-linux-gnu \
	$(INITRAMFS)/usr/lib/x86_64-linux-gnu/dri \
	$(INITRAMFS_EMPTY_DIRS)

# Include test as target if BENCHMARK is not set.
ifeq ($(BENCHMARK), none)
INITRAMFS_ALL_DIRS += $(INITRAMFS)/test
endif

SYSCALL_TEST_DIR := $(INITRAMFS)/opt/syscall_test

.PHONY: all
all: build

$(INITRAMFS)/lib/x86_64-linux-gnu: | $(VDSO_LIB)
	@mkdir -p $@
	@cp -L /lib/x86_64-linux-gnu/libc.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libstdc++.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libm.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libgcc_s.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libpthread.so.0 $@
	@# required for benchmarks
	@cp -L /lib/x86_64-linux-gnu/libcrypto.so.3 $@
	@cp -L /lib/x86_64-linux-gnu/libcrypt.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libdl.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libz.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libmvec.so.1 $@
	@cp -L /usr/local/benchmark/iperf/lib/libiperf.so.0 $@
	@# required for LMbench-network
	@cp -L /lib/x86_64-linux-gnu/libtirpc.so.3 $@
	@cp -L /lib/x86_64-linux-gnu/libgssapi_krb5.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libkrb5.so.3 $@
	@cp -L /lib/x86_64-linux-gnu/libk5crypto.so.3 $@
	@cp -L /lib/x86_64-linux-gnu/libcom_err.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libkrb5support.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libkeyutils.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libresolv.so.2 $@
	@# required for LevelDB-db_bench_sqlite3
	@cp -L /lib/x86_64-linux-gnu/libsqlite3.so $@
	@# required for memcached
	@cp -L /lib/x86_64-linux-gnu/libevent-2.1.so.7 $@
	@# required for VDSO
	@cp -L $(VDSO_LIB) $@
	@# required for netlink test
	@cp -L /lib/x86_64-linux-gnu/libnl-3.so.200 $@
	@cp -L /lib/x86_64-linux-gnu/libnl-route-3.so.200 $@
	@# required for Xorg
	@cp -L /lib/x86_64-linux-gnu/libpixman-1.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libpciaccess.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libXau.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libXdmcp.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libXfont2.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libxshmfence.so.1 $@
	@cp -L /root/ubuntu/dbus/dbus-1.12.20/dbus/.libs/libdbus-1.so.3 $@
	@cp -L /lib/x86_64-linux-gnu/libudev.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libdrm.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libxcvt.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libbsd.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libbz2.so.1.0 $@
	@cp -L /lib/x86_64-linux-gnu/libfontenc.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libfreetype.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libsystemd.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libcap.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libmd.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libpng16.so.16 $@
	@cp -L /lib/x86_64-linux-gnu/libbrotlidec.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libgcrypt.so.20 $@
	@cp -L /lib/x86_64-linux-gnu/liblz4.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/liblzma.so.5 $@
	@cp -L /lib/x86_64-linux-gnu/libzstd.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libbrotlicommon.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libgpg-error.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libGL.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libGLdispatch.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libGLX.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libX11.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libxcb.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libLLVM-15.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libexpat.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libsensors.so.5 $@
	@cp -L /lib/x86_64-linux-gnu/libdrm_radeon.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libelf.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libdrm_amdgpu.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libdrm_nouveau.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libdrm_intel.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libxcb-dri3.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libffi.so.8 $@
	@cp -L /lib/x86_64-linux-gnu/libedit.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libtinfo.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libxml2.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libicuuc.so.70 $@
	@cp -L /lib/x86_64-linux-gnu/libicudata.so.70 $@
	@cp -L /lib/x86_64-linux-gnu/libXi.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libXext.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libXmu.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libXt.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libXrender.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libX11-xcb.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libxcb-present.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libxcb-xfixes.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libxcb-damage.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libSM.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libICE.so.6 $@
	@cp -L /lib/x86_64-linux-gnu/libuuid.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libevdev.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libinput.so.10 $@
	@cp -L /lib/x86_64-linux-gnu/libmtdev.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libwacom.so.9 $@
	@cp -L /lib/x86_64-linux-gnu/libgudev-1.0.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libgobject-2.0.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libpcre.so.3 $@
	@cp -L /lib/x86_64-linux-gnu/libglib-2.0.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libXaw.so.7 $@
	@cp -L /lib/x86_64-linux-gnu/libXpm.so.4 $@
	@cp -L /lib/x86_64-linux-gnu/libXfixes.so.3 $@
	@cp -L /lib/x86_64-linux-gnu/libXinerama.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libXrandr.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libXcursor.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libXcomposite.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libXdamage.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libgdk-3.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libgtk-3.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libgdk_pixbuf-2.0.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libpangocairo-1.0.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libpango-1.0.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libharfbuzz.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libcairo.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libepoxy.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libgio-2.0.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libgmodule-2.0.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libcairo-gobject.so.2 $@
	@cp -L /lib/x86_64-linux-gnu/libatk-1.0.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libatk-bridge-2.0.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libfribidi.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libpangoft2-1.0.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libfontconfig.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libgraphite2.so.3 $@
	@cp -L /lib/x86_64-linux-gnu/libjpeg.so.8 $@
	@cp -L /lib/x86_64-linux-gnu/libthai.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libxkbcommon.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libmount.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libblkid.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/liblz4.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libxcb-shm.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libxcb-render.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libpcre.so.3 $@
	@cp -L /lib/x86_64-linux-gnu/libdatrie.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libatspi.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libwayland-cursor.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libwayland-egl.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libwayland-client.so.0 $@
	@cp -L /lib/x86_64-linux-gnu/libselinux.so.1 $@
	@cp -L /lib/x86_64-linux-gnu/libpcre2-8.so.0 $@

$(VDSO_LIB): | $(VDSO_DIR) $(BINARY_CACHE_DIR)/vdso64.so
	@# TODO: use a custom compiled vdso.so file in the future.
	@cp $(BINARY_CACHE_DIR)/vdso64.so $@

$(VDSO_DIR):
	@mkdir -p $@
	
$(BINARY_CACHE_DIR)/vdso64.so:
	@mkdir -p $(BINARY_CACHE_DIR)
	@$(ATOMIC_WGET) $@ "https://raw.githubusercontent.com/asterinas/linux_vdso/2a6d2db/vdso64.so"

$(INITRAMFS)/lib64:
	@mkdir -p $@
	@cp -L /lib64/ld-linux-x86-64.so.2 $@

$(INITRAMFS)/etc:
	@mkdir -p $@
	@cp $(CUR_DIR)/etc/* $@
	@cp -L /etc/machine-id $@/machine-id

$(INITRAMFS)/var: | $(INITRAMFS)
	@mkdir -p $@

$(INITRAMFS)/var/run: | $(INITRAMFS)/run $(INITRAMFS)/var
	@rm -rf $@
	@ln -sf ../run $@

$(INITRAMFS)/run:
	@mkdir -p $@
	@mkdir -p $@/dbus
	@chown messagebus:messagebus $@/dbus
	@chmod 755 $@/dbus

$(INITRAMFS)/run/user/1000:
	@mkdir -p $@

# Install busybox into /bin and /usr/bin.
$(INITRAMFS)/bin:
	@mkdir -p $@
	@/bin/busybox --install -s $@

$(INITRAMFS)/sbin:
	@mkdir -p $@
	@cp /sbin/ldconfig $@
	@cp /sbin/ldconfig.real $@

$(INITRAMFS)/usr/bin: | $(INITRAMFS)/bin
	@mkdir -p $@
	@cp /usr/bin/busybox $@
	@cp /usr-xorg/bin/Xvfb $@
	@cp /usr-xorg/bin/Xorg $@
	@cp /usr-xorg/bin/gtf $@
	@cp /usr/bin/setxkbmap $@
	@cp /usr/bin/xkbbell $@
	@cp /usr/bin/xkbcomp $@
	@cp /usr/bin/xkbevd $@
	@cp /usr/bin/xkbprint $@
	@cp /usr/bin/xkbvleds $@
	@cp /usr/bin/xkbwatch $@
	@cp /usr/bin/xeyes $@
	@cp /usr/bin/xlogo $@
	@cp /usr/bin/xmessage $@
	@echo "#!/bin/sh" > $@/run_xorg.sh
	@echo "./usr/bin/Xorg :0 -modulepath /usr/lib/xorg/modules -config /etc/X11/xorg.conf.d/10-fbdev.conf -logverbose 6 -logfile ~/xorg_debug.log -novtswitch -keeptty -keyboard "keyboard" -pointer "mouse0" &" >> $@/run_xorg.sh
	@chmod +x $@/run_xorg.sh
	@cp /root/ubuntu/dbus/dbus-1.12.20/setup_dbus.sh $@/setup_dbus.sh
	@echo "/usr/bin/dbus-send --system --dest=org.freedesktop.DBus --print-reply /org/freedesktop/DBus org.freedesktop.DBus.ListNames" > $@/run_dbus-send.sh
	@echo "/usr/bin/dbus-send --session --dest=org.freedesktop.DBus --print-reply /org/freedesktop/DBus org.freedesktop.DBus.ListNames" >> $@/run_dbus-send.sh
	@chmod +x $@/run_dbus-send.sh
	@cp /root/ubuntu/dbus/dbus-1.12.20/bus/.libs/dbus-daemon $@/dbus-daemon
	@cp /root/ubuntu/dbus/dbus-1.12.20/tools/.libs/dbus-uuidgen $@/dbus-uuidgen
	@cp /root/ubuntu/dbus/dbus-1.12.20/tools/.libs/dbus-launch $@/dbus-launch
	@cp /root/ubuntu/dbus/dbus-1.12.20/tools/.libs/dbus-send $@/dbus-send
	@cp /root/ubuntu/dbus/dbus-1.12.20/tools/.libs/dbus-monitor $@/dbus-monitor
	@cp /root/ubuntu/dbus/dbus-1.12.20/tools/dbus-cleanup-sockets $@/dbus-cleanup-sockets
	@cp /usr/bin/gtk3-demo $@

$(INITRAMFS)/usr/local:
	@mkdir -p $@
	@cp -r /usr/local/nginx $@
	@cp -r /usr/local/redis $@
	@cp -r /usr/local/memcached $@

$(INITRAMFS)/usr/share:
	@mkdir -p $@
	@cp -r /usr/share/fonts $@
	@cp -r /usr/share/X11 $@
	@mkdir -p $@/dbus-1
	@cp /root/ubuntu/dbus/dbus-1.12.20/bus/system.conf $@/dbus-1/system.conf
	@cp /root/ubuntu/dbus/dbus-1.12.20/bus/session.conf $@/dbus-1/session.conf
	@mkdir -p $@/glib-2.0/schemas
	@cp -r /usr/share/glib-2.0/schemas/* $@/glib-2.0/schemas/
	@mkdir -p $@/themes/Default/gtk-3.0
	@cp -r /usr/share/themes/Default/gtk-3.0/* $@/themes/Default/gtk-3.0/
	@mkdir -p $@/icons/Adwaita
	@cp -r /usr/share/icons/Adwaita/* $@/icons/Adwaita/
	@cp -r /usr/share/locale $@
	@cp -r /usr/share/gtk-3.0 $@

$(INITRAMFS)/etc/X11:
	@mkdir -p $@
	@cp -r /etc/X11/xorg.conf.d $@

$(INITRAMFS)/etc/dbus-1:
	@mkdir -p $@
	@cp -r /etc/dbus-1/system.d $@
	@cp -r /etc/dbus-1/session.d $@
	@cp -r /root/ubuntu/dbus/dbus-1.12.20/bus/legacy-config/system.conf $@
	@cp -r /root/ubuntu/dbus/dbus-1.12.20/bus/session.conf $@

$(INITRAMFS)/usr/lib:
	@mkdir -p $@
	@cp -r /usr/lib/xorg $@
	@cp -r /usr-xorg/lib/x86_64-linux-gnu/xorg/modules/ $@/xorg/modules/

$(INITRAMFS)/usr/lib/x86_64-linux-gnu:
	@mkdir -p $@
	@cp -L /usr/lib/x86_64-linux-gnu/libxkbfile.so.1 $@
	@cp -L /usr/lib/x86_64-linux-gnu/libglapi.so.0 $@
	@cp -r /usr/lib/x86_64-linux-gnu/gtk-3.0/* $@/gtk-3.0/

$(INITRAMFS)/usr/lib/x86_64-linux-gnu/dri:
	@mkdir -p $@
	@cp -L /usr/lib/x86_64-linux-gnu/dri/swrast_dri.so $@

.PHONY: $(INITRAMFS)/test
$(INITRAMFS)/test:
	@$(MAKE) --no-print-directory -C apps

$(INITRAMFS)/benchmark: | $(INITRAMFS)/benchmark/bin
	@cp -rf $(CUR_DIR)/benchmark/* $@

$(INITRAMFS)/benchmark/bin:
	@mkdir -p $@
	@cp /usr/local/benchmark/sysbench/bin/sysbench $@
	@cp /usr/local/benchmark/iperf/bin/iperf3 $@
	@cp /usr/local/benchmark/membench/membench $@
	@cp /usr/local/benchmark/iozone $@
	@cp /usr/local/benchmark/my_gtk_test $@
	@cp /usr/local/benchmark/gtk_test $@
	@cp -r /usr/local/benchmark/unixbench $@
	@cp -r /usr/local/benchmark/lmbench $@
	@cp /usr/local/benchmark/fio/bin/fio $@
	@cp /usr/local/benchmark/sqlite-speedtest1 $@
	@cp /usr/local/leveldb/benchmark/db_bench $@
	@mv $@/db_bench $@/db_bench_leveldb
	@cp /usr/local/leveldb/benchmark/db_bench_sqlite3 $@
	@cp /usr/local/benchmark/hackbench $@
	@cp /usr/local/benchmark/schbench $@
	@if [ -f /root/tz_utils/aster_gui/framebuffer-tetris/main ]; then \
		mkdir -p $@/gui; \
		cp /root/tz_utils/aster_gui/framebuffer-tetris/main $@/tetris; \
		cp /root/tz_utils/aster_gui/framebuffer-tetris/main $@/gui/tetris; \
	fi

# Make necessary directories.
$(INITRAMFS_EMPTY_DIRS):
	@mkdir -p $@

.PHONY: $(SYSCALL_TEST_DIR)
$(SYSCALL_TEST_DIR):
	@$(MAKE) --no-print-directory -C syscall_test

.PHONY: $(INITRAMFS_IMAGE)
$(INITRAMFS_IMAGE): $(INITRAMFS_FILELIST)
	@if ! cmp -s $(INITRAMFS_FILELIST) $(INITRAMFS_FILELIST).previous || ! test -f $@; then \
		echo "Generating the initramfs image..."; \
		cp -f $(INITRAMFS_FILELIST) $(INITRAMFS_FILELIST).previous; \
		( \
			cd $(INITRAMFS); \
			# `$(INITRAMFS_FILELIST)` contains files' last modification \
			# time in the first column, and files' relative path to \
			# `$(INITRAMFS)` in the second column. This prunes the first \
			# column and passes the second column to `cpio`. \
			cut -d " " -f 2- $(INITRAMFS_FILELIST) | \
				cpio -o -H newc | \
					if [ "$(INITRAMFS_SKIP_GZIP)" != 1 ]; then \
						gzip; \
					else \
						cat; \
					fi \
		) > $@; \
	fi

.PHONY: $(INITRAMFS_FILELIST)
# If the BUILD_SYSCALL_TEST variable is set, we should depend on the
# sub make output to do incremental building.
ifeq ($(BUILD_SYSCALL_TEST), 1)
$(INITRAMFS_FILELIST): | $(INITRAMFS_ALL_DIRS) $(SYSCALL_TEST_DIR)
else
$(INITRAMFS_FILELIST): | $(INITRAMFS_ALL_DIRS)
endif
	@(cd $(INITRAMFS); find . -printf "%T@ %p\n") > $(INITRAMFS_FILELIST)

$(EXT2_IMAGE):
	@dd if=/dev/zero of=$(EXT2_IMAGE) bs=2G count=1
	@mke2fs $(EXT2_IMAGE)

$(EXFAT_IMAGE):
	@fallocate -l 64M $(EXFAT_IMAGE)
	@mkfs.exfat $(EXFAT_IMAGE)

.PHONY: build
build: $(INITRAMFS_IMAGE) $(EXT2_IMAGE) $(EXFAT_IMAGE)

.PHONY: format
format:
	@$(MAKE) --no-print-directory -C apps format

.PHONY: check
check:
	@$(MAKE) --no-print-directory -C apps check

.PHONY: clean
clean:
	@rm -rf $(BUILD_DIR)
