import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt

original_results = pd.read_csv("output/data/original_inference_times.csv", low_memory=False)
adapted_results = pd.read_csv("output/data/adapted_inference_times.csv", low_memory=False)

# bars = pd.DataFrame(columns=["BinSize", "Location", "Runtime"])
# for bin_size in [0.5, 1.0, 1.5, 2.5, 5.0, 7.5, 10.0]:
#     for location in [10]:
#         subset = results[results["BinSize"] == bin_size]
#         subset = subset[subset["Location"] == location]
#         subset = subset[subset["Time"] < 20]

#         data = pd.DataFrame(data={"BinSize": bin_size, "Location": location, "Adapted": False, "Runtime": subset["Runtime"].mean()}, index=[0])
#         bars = pd.concat([bars, data], ignore_index=True)

#         subset = results[results["BinSize"] == bin_size]
#         subset = subset[subset["Location"] == location]
#         subset = subset[subset["Time"] > 20]

#         data = pd.DataFrame(data={"BinSize": bin_size, "Location": location, "Adapted": True, "Runtime": subset["Runtime"].mean()}, index=[0])
#         bars = pd.concat([bars, data], ignore_index=True)

# results["Runtime"] = 1.0 / results["Runtime"]  # Show inference frequency instead of time
plot = sns.lineplot(data=original_results, x="BinSize", y="Runtime", label="Original")
# plot = sns.lineplot(data=original_results, x="Time", y="Runtime", hue="BinSize")

for location in [5, 10]:
    distribution_results = adapted_results[adapted_results["Location"] == location]
    sns.lineplot(data=distribution_results, x="BinSize", y="Runtime", label=r'$\mathcal{N}(' + f"{location}" + r', 3)$')

plt.gcf().tight_layout()
sns.despine(top=True, right=True)

plot.set_xlabel("Bin size / Hz", fontsize=20)
plot.set_ylabel("Inference Time / s", fontsize=20)
plot.tick_params(labelsize=15)
# plot.set_ylim([0.0, 0.1])
# plot.legend(title="Mean FoC", fontsize=15)
plot.set_yscale("log")
# plot.set_aspect(8.0 / 10.0)

plot.get_figure().savefig("output/plots/time_plot.pdf", bbox_inches='tight')
