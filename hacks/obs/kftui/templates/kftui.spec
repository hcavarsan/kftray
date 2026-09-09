Name:           kftui
Version:        {{VERSION}}
Release:        1%{?dist}
Summary:        Kubernetes port-forwarding CLI manager
License:        GPL-3.0-only
URL:            https://github.com/hcavarsan/kftray
Source0:        kftui_{{VERSION}}.orig.tar.gz
ExclusiveArch:  x86_64 aarch64

%description
KFtui - Kubernetes port-forwarding CLI manager

%prep
%setup -q

%build

%install
mkdir -p %{buildroot}%{_bindir}
%ifarch x86_64
install -Dm755 kftui_linux_amd64 %{buildroot}%{_bindir}/kftui
%endif
%ifarch aarch64
install -Dm755 kftui_linux_arm64 %{buildroot}%{_bindir}/kftui
%endif

%files
%{_bindir}/kftui
