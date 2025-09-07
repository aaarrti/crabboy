import json


def main():

    with open("data/Opcodes.json") as f:
        opcode: dict = json.load(f)

    with open("src/cpu.rs") as f:

        old_lines = f.readlines()

        new_lines = []

        for line in old_lines:
            new_lines.append(line)

            if "codegen-start" in line:

                for unprefixed, details in opcode["unprefixed"].items():

                    new_lines.append(f"{unprefixed} => " + "{\n")

                    new_lines.append(f"// {details['operands']} \n")
                    new_lines.append(f"// {details['flags']} \n")

                    operands = [i["name"] for i in details["operands"]]

                    new_lines.append(
                        'tracing::trace!("'
                        + details["mnemonic"]
                        + " "
                        + ", ".join(operands)
                        + '");\n'
                    )

                    new_lines.append(
                        'panic!("opcode '
                        + details["mnemonic"]
                        + " "
                        + ", ".join(operands)
                        + ' not implemented");\n'
                    )

                    cycles = details["cycles"]
                    new_lines.append("// cycles: " + str(cycles) + "\n")

                    new_lines.append("}\n")

            if "codegen-prefix-start" in line:
                for cbprefixed, details in opcode["cbprefixed"].items():

                    new_lines.append(f"{cbprefixed} => " + "{\n")

                    new_lines.append(f"// {details['operands']} \n")
                    new_lines.append(f"// {details['flags']} \n")

                    operands = [i["name"] for i in details["operands"]]

                    new_lines.append(
                        'tracing::trace!("'
                        + details["mnemonic"]
                        + " "
                        + ", ".join(operands)
                        + '");\n'
                    )

                    new_lines.append(
                        'panic!("opcode '
                        + details["mnemonic"]
                        + " "
                        + ", ".join(operands)
                        + ' not implemented")\n'
                    )

                    new_lines.append("// cycles: " + str(details["cycles"]) + "\n")

                    new_lines.append("}\n")

    with open("src/cpu.rs", "w+") as f:
        for line in new_lines:
            f.write(line)


if __name__ == "__main__":
    main()
