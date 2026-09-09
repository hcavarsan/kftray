Name:           kftray
Version:        {{VERSION}}
Release:        1%{?dist}
Summary:        Kubernetes port-forwarding GUI manager
License:        GPL-3.0-only
URL:            https://github.com/hcavarsan/kftray
Source0:        kftray_{{VERSION}}.orig.tar.gz
ExclusiveArch:  x86_64 aarch64
Requires:       fuse3

%global __strip /bin/true
%global debug_package %{nil}

%description
KFtray - Kubernetes port-forwarding GUI manager

%prep
%setup -q

%build

%install
mkdir -p %{buildroot}%{_bindir}
variant=
if getconf GNU_LIBC_VERSION | awk -F '[ .]' '{ exit !($2 > 2 || ($2 == 2 && $3 >= 39)) }'; then
    variant=newer-glibc_
fi
%ifarch x86_64
install -Dm755 "kftray_{{VERSION}}_${variant}amd64.AppImage" %{buildroot}%{_bindir}/kftray
%endif
%ifarch aarch64
install -Dm755 "kftray_{{VERSION}}_${variant}aarch64.AppImage" %{buildroot}%{_bindir}/kftray
%endif

%files
%{_bindir}/kftray
