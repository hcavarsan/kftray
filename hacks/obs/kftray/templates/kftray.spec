Name:           kftray
Version:        {{VERSION}}
Release:        1%{?dist}
Summary:        Kubernetes port-forwarding GUI manager
License:        GPL-3.0
URL:            https://github.com/hcavarsan/kftray
Source0:        kftray-{{VERSION}}-amd64.AppImage
Source1:        kftray-{{VERSION}}-aarch64.AppImage

%description
KFtray - Kubernetes port-forwarding GUI manager

%prep

%build

%install
mkdir -p %{buildroot}%{_bindir}
%ifarch x86_64
install -Dm755 %{SOURCE0} %{buildroot}%{_bindir}/kftray
%endif
%ifarch aarch64
install -Dm755 %{SOURCE1} %{buildroot}%{_bindir}/kftray
%endif

%files
%{_bindir}/kftray