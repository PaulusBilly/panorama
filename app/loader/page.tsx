import type { Metadata } from "next";
import { LoaderAnimationDrafts } from "./LoaderAnimationDrafts";
import "./loader-drafts.css";

export const metadata: Metadata = {
  title: "Loader animation drafts | Panorama",
};

export default function LoaderDraftPage() {
  return <LoaderAnimationDrafts />;
}
