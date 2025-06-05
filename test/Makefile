# SPDX-License-Identifier: MPL-2.0

ARCH ?= x86_64
SMP ?= 1
VERBOSE ?= 1
SYSCALL_TEST_SUITE ?= ltp
SYSCALL_TEST_WORKDIR ?= /tmp
ENABLE_BASIC_TEST ?= false
# Set Nix's cached tarballs to be live for a longer period of time (30 days) to avoid network traffics.
# Nix's default value is rather small (1 hour or 3600 seconds).
NIXPKGS_CACHE_TTL := 2592000 # In seconds
MKFILE_PATH := $(abspath $(lastword $(MAKEFILE_LIST)))
CUR_DIR := $(patsubst %/,%,$(dir $(MKFILE_PATH)))
BUILD_DIR := $(CUR_DIR)/build
INITRAMFS := $(BUILD_DIR)/initramfs
INITRAMFS_SKIP_GZIP ?= 0
ifeq ($(INITRAMFS_SKIP_GZIP),1)
INITRAMFS_IMAGE := $(BUILD_DIR)/initramfs.cpio
INITRAMFS_COMPRESSED := false
else
INITRAMFS_IMAGE := $(BUILD_DIR)/initramfs.cpio.gz
INITRAMFS_COMPRESSED := true
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

# Include benchmark, if BENCHMARK is set.
ifeq ($(BENCHMARK), none)
ENABLE_BENCHMARK = false
else
ENABLE_BENCHMARK = true
endif

# Include syscall_test, if BUILD_SYSCALL_TEST is set.
ifeq ($(BUILD_SYSCALL_TEST), 1)
ENABLE_SYSCALL_TEST = true
else
ENABLE_SYSCALL_TEST = false
endif

# Decreases the level of verbosity of diagnostic messages from Nix.
ifeq ($(VERBOSE), 0)
NIX_QUIET = --quiet -Q
endif

.PHONY: all
all: $(INITRAMFS_IMAGE)

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

$(INITRAMFS)/var/run: | $(INITRAMFS)/run
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
	@cp /usr/bin/Xvfb $@
	@cp /usr/bin/Xorg $@
	@cp /usr/bin/gtf $@
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

$(INITRAMFS)/usr/lib/x86_64-linux-gnu:
	@mkdir -p $@
	@cp -L /usr/lib/x86_64-linux-gnu/libxkbfile.so.1 $@
	@cp -L /usr/lib/x86_64-linux-gnu/libglapi.so.0 $@

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
$(INITRAMFS_IMAGE): $(INITRAMFS)
	@nix-build \
		--tarball-ttl $(NIXPKGS_CACHE_TTL) \
		--argstr target $(ARCH) \
		--arg enableBasicTest $(ENABLE_BASIC_TEST) \
		--arg enableBenchmark $(ENABLE_BENCHMARK) \
		--arg enableSyscallTest $(ENABLE_SYSCALL_TEST) \
		--argstr syscallTestSuite $(SYSCALL_TEST_SUITE) \
		--argstr syscallTestWorkDir $(SYSCALL_TEST_WORKDIR) \
		--arg initramfsCompressed $(INITRAMFS_COMPRESSED) \
		--arg smp $(SMP) \
		--out-link $@ \
		nix -A initramfs-image

.PHONY: $(INITRAMFS)
$(INITRAMFS):
	@nix-build \
		--tarball-ttl $(NIXPKGS_CACHE_TTL) \
		--argstr target $(ARCH) \
		--arg enableBasicTest $(ENABLE_BASIC_TEST) \
		--arg enableBenchmark $(ENABLE_BENCHMARK) \
		--arg enableSyscallTest $(ENABLE_SYSCALL_TEST) \
		--argstr syscallTestSuite $(SYSCALL_TEST_SUITE) \
		--argstr syscallTestWorkDir $(SYSCALL_TEST_WORKDIR) \
		--arg smp $(SMP) \
		--out-link $@ \
		nix -A initramfs

# Prebuild x86_64 packages
x86_64_pkgs:
	@nix-build \
		nix/default.nix \
		--tarball-ttl $(NIXPKGS_CACHE_TTL) \
		$(NIX_QUIET) \
		--argstr target x86_64 \
		--arg enableBenchmark true \
		--arg enableSyscallTest true \
		--out-link /nix/var/nix/gcroots/auto/x86_64-pkgs \
		-A busybox \
		-A apps.package \
		-A benchmark.fio \
		-A benchmark.hackbench \
		-A benchmark.iperf3 \
		-A benchmark.lmbench \
		-A benchmark.memcached \
		-A benchmark.nginx \
		-A benchmark.redis \
		-A benchmark.schbench \
		-A benchmark.sqlite-speedtest1 \
		-A benchmark.sysbench \
		-A syscall.ltp

# Prebuild riscv64 packages
# Note: This may cause GitHub Runner ResourceExhausted errors when publish nix docker image.
# If such errors occur, try building each package individually and clear the build cache.
riscv64_pkgs:
	@nix-build \
		nix/default.nix \
		--tarball-ttl $(NIXPKGS_CACHE_TTL) \
		$(NIX_QUIET) \
		--argstr target riscv64 \
		--arg enableBenchmark true \
		--arg enableSyscallTest true \
		--out-link /nix/var/nix/gcroots/auto/riscv64-pkgs \
		-A busybox \
		-A apps.package \
		-A benchmark.fio \
		-A benchmark.hackbench \
		-A benchmark.iperf3 \
		-A benchmark.lmbench \
		-A benchmark.memcached \
		-A benchmark.nginx \
		-A benchmark.redis \
		-A benchmark.schbench \
		-A benchmark.sqlite-speedtest1 \
		-A syscall.ltp

general_pkgs:
	@nix-build \
		nix/default.nix \
		--tarball-ttl $(NIXPKGS_CACHE_TTL) \
		$(NIX_QUIET) \
		--out-link /nix/var/nix/gcroots/auto/general \
		-A linux_vdso \
		-A apps.mongoose_src

install_host_pkgs:
	@nix-env \
		--file nix/default.nix \
		--tarball-ttl $(NIXPKGS_CACHE_TTL) \
		--install \
		-A apacheHttpd \
		-A iperf3 \
		-A libmemcached \
		-A lmbench \
		-A redis

nix_gc:
	@nix-collect-garbage -d
	@nix-store --optimise

$(EXT2_IMAGE):
	@mkdir -p $(BUILD_DIR)
	@dd if=/dev/zero of=$(EXT2_IMAGE) bs=2G count=1
	@mke2fs $(EXT2_IMAGE)

$(EXFAT_IMAGE):
	@mkdir -p $(BUILD_DIR)
	@fallocate -l 64M $(EXFAT_IMAGE)
	@mkfs.exfat $(EXFAT_IMAGE)

.PHONY: format
format:
	@$(MAKE) --no-print-directory -C src/apps format
	@nixfmt nix

.PHONY: check
check:
	@$(MAKE) --no-print-directory -C src/apps check

.PHONY: clean
clean:
	@rm -rf $(BUILD_DIR)
