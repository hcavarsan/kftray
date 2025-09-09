Name:           kftui
Version:        {{VERSION}}
Release:        1%{?dist}
Summary:        Kubernetes port-forwarding CLI manager
License:        GPL-3.0
URL:            https://github.com/hcavarsan/kftray
Source0:        kftui-{{VERSION}}-amd64
Source1:        kftui-{{VERSION}}-arm64

%description
KFtui - Kubernetes port-forwarding CLI manager

%prep

%build

%install
%ifarch x86_64
install -Dm755 %{SOURCE0} %{buildroot}%{_bindir}/kftui
%endif
%ifarch aarch64
install -Dm755 %{SOURCE1} %{buildroot}%{_bindir}/kftui
%endif

%files
%{_bindir}/kftui