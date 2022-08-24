{ pkgs ? import <nixpkgs> {}, ... }:

pkgs.mkShell {
    nativeBuildInputs = with pkgs; [ 
        podman
        
        asciidocFull asciidoctor pandoc rubyPackages.rouge ruby cmake wrapGAppsHook gdk-pixbuf cairo pango libxml2 bison flex python
    ];
}

# asciidoctor -b pdf -r asciidoctor-pdf -a optimize -a compress -r asciidoctor-mathematical ./document-reponse.adoc