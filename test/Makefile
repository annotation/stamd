# Downloads Brieven van Hooft data for test, see 
# https://github.com/knaw-huc/brieven-van-hooft-pipeline
# for notes on the data and its licensing

.PHONY: test

test: docroot/hoof001hwva.annotationstore.stam.json
	cargo run --release -- --debug --unload-time 120 --ns "brieven-van-hooft-metadata:http://127.0.0.1:8080/hoof001hwva/datasets/brieven-van-hooft-metadata/" -d docroot -e annotationstore.stam.json

docroot:
	mkdir -p $@

docroot/hoof001hwva.annotationstore.stam.json: docroot docroot/hoof001hwva02.txt docroot/hoof001hwva03.txt docroot/hoof001hwva04.txt
	curl https://download.anaproy.nl/hoof001hwva.output.store.stam.json > $@

docroot/hoof001hwva02.txt:
	curl https://www.dbnl.org/nieuws/text.php?id=hoof001hwva02 > $@
	@echo "checksum ok?"
	CHECKSUM="$$(sha256sum $@ | cut -d' ' -f 1)" && [ "$$CHECKSUM" = "5f0df29a5ea14e87bc66c3a8e8012ec966a8a948b709cc80504c6fb5c2e9d82b" ]

docroot/hoof001hwva03.txt:
	curl https://www.dbnl.org/nieuws/text.php?id=hoof001hwva03 > $@
	@echo "checksum ok?"
	CHECKSUM="$$(sha256sum $@ | cut -d' ' -f 1)" && [ "$$CHECKSUM" = "4c0a23a238b6da382c6a0c5334a867d8e3ef4cb081aae37c5104cf612cbeb64a" ]

docroot/hoof001hwva04.txt:
	curl https://www.dbnl.org/nieuws/text.php?id=hoof001hwva04 > $@
	@echo "checksum ok?"
	CHECKSUM="$$(sha256sum $@ | cut -d' ' -f 1)" && [ "$$CHECKSUM" = "6a2f9c4454f0db71a84c774418edaa9adc4ee19a5b3da00f051dd8c6b2f691df" ]




